// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! # libp2p-dns
//!
//! This crate provides the type `DnsConfig` that allows one to resolve the `/dns4/` and `/dns6/`
//! components of multiaddresses.
//!
//! ## Usage
//!
//! In order to use this crate, create a `DnsConfig` with one of its constructors and pass it an
//! implementation of the `Transport` trait.
//!
//! Whenever we want to dial an address through the `DnsConfig` and that address contains a
//! `/dns/`, `/dns4/`, or `/dns6/` component, a DNS resolve will be performed and the component
//! will be replaced with `/ip4/` and/or `/ip6/` components.
//!
use async_trait::async_trait;
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::TransportError,
};
use log::{error, trace};
use std::{error, fmt, io, net::ToSocketAddrs};

/// Represents the configuration for a DNS transport capability of libp2p.
///
/// This struct implements the `Transport` trait and holds an underlying transport. Any call to
/// `dial` with a multiaddr that contains `/dns/`, `/dns4/`, or `/dns6/` will be first be resolved,
/// then passed to the underlying transport.
///
/// Listening is unaffected.
#[derive(Clone)]
pub struct DnsConfig<T> {
    /// Underlying transport to use once the DNS addresses have been resolved.
    inner: T,
}

impl<T> DnsConfig<T> {
    /// Creates a new configuration object for DNS.
    pub fn new(inner: T) -> Self {
        DnsConfig{inner}
    }
}

impl<T> fmt::Debug for DnsConfig<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("DnsConfig").field(&self.inner).finish()
    }
}

#[async_trait]
impl<T> Transport for DnsConfig<T>
where
    T: Transport + Send + 'static,
{
    type Output = T::Output;
    type Listener = T::Listener;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        self.inner.listen_on(addr)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        // one step complete all task
        let mut iter = addr.iter();
        let proto = iter.find_map(|x| match x {
            Protocol::Dns(name) => Some((name, true, true)),
            Protocol::Dns4(name) => Some((name, true, false)),
            Protocol::Dns6(name) => Some((name, false, true)),
            _ => None,
        });

        let index = addr.iter().count() - iter.count() - 1;

        // As an optimization, we immediately pass through if no component of the address contain
        // a DNS protocol.
        let (name, dns4, dns6) = match proto {
            Some((name, dns4, dns6)) => (name, dns4, dns6),
            None => {
                trace!("Pass-through address without DNS: {}", addr);
                return self.inner.dial(addr).await;
            }
        };

        let name = name.to_string();
        let to_resolve = format!("{}:0", name);


        let list = to_resolve[..].to_socket_addrs().map_err(|_| {
            error!("DNS resolver crashed");
            TransportError::ResolveFail(name.clone())
        })?;
        let list = list.map(|s| s.ip()).collect::<Vec<_>>();

        let outcome = list.into_iter()
            .filter_map(|addr| {
                if (dns4 && addr.is_ipv4()) || (dns6 && addr.is_ipv6()) {
                    Some(Protocol::from(addr))
                } else {
                    None
                }
            })
            .next()
            .ok_or_else(|| TransportError::ResolveFail(name.clone()))?;


        // let iter = addr.iter().map( |x| match x {
        //     Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) => {
        //         outcome.clone()
        //     }
        //     proto => proto,
        // });
        //
        // let addr = Multiaddr::from_iter(iter);

        if let Some(addr) = addr.replace(index, |_| Some(outcome.clone())) {
            return self.inner.dial(addr).await;
        }

        Err(TransportError::ResolveFail(name))
    }
}

/// Error that can be generated by the DNS layer.
#[derive(Debug)]
pub enum DnsErr {
    /// Failed to find any IP address for this DNS address.
    ResolveFail(String),
    /// Error while resolving a DNS address.
    ResolveError {
        domain_name: String,
        error: io::Error,
    },
    /// Found an IP address, but the underlying transport doesn't support the multiaddr.
    MultiaddrNotSupported,
}

impl fmt::Display for DnsErr
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsErr::ResolveFail(addr) => write!(f, "Failed to resolve DNS address: {:?}", addr),
            DnsErr::ResolveError { domain_name, error } => {
                write!(f, "Failed to resolve DNS address: {:?}; {:?}", domain_name, error)
            },
            DnsErr::MultiaddrNotSupported => write!(f, "Resolve multiaddr not supported"),
        }
    }
}

impl error::Error for DnsErr
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DnsErr::ResolveFail(_) => None,
            DnsErr::ResolveError { error, .. } => Some(error),
            DnsErr::MultiaddrNotSupported => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DnsConfig;
    use libp2p_tcp::TcpConfig;
    use multiaddr::Multiaddr;
    use libp2p_core::Transport;
    use libp2p_core::transport::TransportListener;
    use async_std::task;
    use libp2p_traits::{ReadEx, WriteEx};

    #[test]
    fn basic_resolve_v4() {
        task::block_on(async move {
            let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8384".parse().unwrap();
            let addr: Multiaddr = "/dns4/example.com/tcp/8384".parse().unwrap();
            let transport = DnsConfig::new(TcpConfig::default());
            let client = transport.clone();

            let msg = b"Hello World";

            let handle = task::spawn(async move {
                let mut listener = transport.listen_on(listen_addr).unwrap();
                let mut conn =  listener.accept().await.unwrap();

                let mut buf = vec![0; msg.len()];
                conn.read_exact2(&mut buf).await.expect("server read exact");

                assert_eq!(&msg[..], &buf[..]);

                conn.close2().await.expect("server close connection");
            });


            let mut conn = client.dial(addr).await.expect("client dial");
            conn.write_all2(&msg[..]).await.expect("client write all");
            conn.close2().await.expect("client close connection");


            handle.await;
        });
    }

    #[test]
    fn basic_resolve_v6() {
        task::block_on(async move {
            let listen_addr: Multiaddr = "/ip6/::1/tcp/8384".parse().unwrap();
            let addr: Multiaddr = "/dns6/example.com/tcp/8384".parse().unwrap();
            let transport = DnsConfig::new(TcpConfig::default());
            let client = transport.clone();

            let msg = b"Hello World";

            let handle = task::spawn(async move {
                let mut listener = transport.listen_on(listen_addr).expect("S listen");
                let mut conn =  listener.accept().await.unwrap();

                let mut buf = vec![0; msg.len()];
                conn.read_exact2(&mut buf).await.expect("S read exact");

                assert_eq!(&msg[..], &buf[..]);

                conn.close2().await.expect("S close connection");
            });


            let mut conn = client.dial(addr).await.expect("C dial");
            conn.write_all2(&msg[..]).await.expect("C write all");
            conn.close2().await.expect("C close connection");

            handle.await;
        });
    }
}
