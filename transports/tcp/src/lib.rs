// Copyright 2018 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
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

//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! # Usage
//!
//! This crate provides `TcpConfig`
//!
//! The `TcpConfig` struct implements the `Transport` trait of the
//! `core` library. See the documentation of `core` and of libp2p in general to learn how to
//! use the `Transport` trait.

use async_trait::async_trait;
use futures::future::Either;
use futures::prelude::*;
use futures::FutureExt;
use if_addrs::{get_if_addrs, IfAddr};
use if_watch::{IfEvent, IfWatcher};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use socket2::{Domain, Socket, Type};
use std::{
    convert::TryFrom,
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};

use libp2prs_core::transport::{ConnectionInfo, IListener, ITransport, ListenerEvent};
use libp2prs_core::{
    multiaddr::{protocol, protocol::Protocol, Multiaddr},
    transport::{TransportError, TransportListener},
    Transport,
};
use libp2prs_runtime::net::{TcpListener, TcpStream};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
#[derive(Debug, Clone, Default)]
pub struct TcpConfig {
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig { ttl: None, nodelay: None }
    }

    /// Sets the TTL to set for opened sockets.
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Sets the `TCP_NODELAY` to set for opened sockets.
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }
}

#[async_trait]
impl Transport for TcpConfig {
    type Output = TcpTransStream;
    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let socket = if socket_addr.is_ipv4() {
            Socket::new(Domain::ipv4(), Type::stream(), Some(socket2::Protocol::tcp()))?
        } else {
            let s = Socket::new(Domain::ipv6(), Type::stream(), Some(socket2::Protocol::tcp()))?;
            s.set_only_v6(true)?;
            s
        };
        if cfg!(target_family = "unix") {
            socket.set_reuse_address(true)?;
        }
        socket.bind(&socket_addr.into())?;
        socket.listen(1024)?; // we may want to make this configurable

        let listener = <TcpListener>::try_from(socket.into_tcp_listener()).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // retrieve the local address/port from Socket
        // local address will remain unspecified if the socket_addr is unspecified
        // but the port will be a random one which is picked by the OS kernel
        // therefore we have to fetch the correct port...
        // think about the case of "/ip4/0.0.0.0/tcp/0"
        let local_addr = listener.local_addr()?;
        let port = local_addr.port();

        log::debug!("Listening on {}", local_addr);

        // if address unspecified, set to None. address will be reported later via
        // ListenerEvent::AddressAdded
        let listen_address = if local_addr.ip().is_unspecified() {
            None
        } else {
            Some(sock_to_multiaddr(local_addr))
        };

        // let addrs = host_addresses(port)?;
        // debug!("Listening on {:?}", addrs.iter().map(|(_, _, ma)| ma).collect::<Vec<_>>());

        let listener = TcpTransListener {
            inner: listener,
            port,
            first: true,
            watcher: None,
            listen_address,
            config: self.clone(),
        };

        Ok(Box::new(listener))
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                log::debug!("Instantly refusing dialing {}, as it is invalid", addr);
                return Err(TransportError::IoError(io::ErrorKind::ConnectionRefused.into()));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        log::debug!("Dialing {}", addr);

        let stream = TcpStream::connect(&socket_addr).await?;
        apply_config(&self, &stream)?;

        // figure out the local sock address
        let local_addr = stream.local_addr()?;
        let la = sock_to_multiaddr(local_addr);
        Ok(TcpTransStream {
            inner: stream,
            la,
            ra: addr,
        })
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        vec![protocol::TCP]
    }
}

/// Wraps around a `TcpListener`.
#[derive(Debug)]
pub struct TcpTransListener {
    inner: TcpListener,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// Indicates the first time of being polled/accepted.
    first: bool,
    /// The interface watcher for address changes(interface up/down).
    watcher: Option<IfWatcher>,
    /// The listened addresses. This address is constructed from the listener.local_addr.
    /// `Some` when the local_addr is not unspecified
    /// `None` when the local_addr is unspecified, i.e., 0.0.0.0
    listen_address: Option<Multiaddr>,
    /// Original configuration.
    config: TcpConfig,
}

#[async_trait]
impl TransportListener for TcpTransListener {
    type Output = TcpTransStream;
    async fn accept(&mut self) -> Result<ListenerEvent<Self::Output>, TransportError> {
        if self.first {
            self.first = false;
            if self.listen_address.is_none() {
                // at first time, we have to initialize the watcher
                log::info!("initializing if-watcher...");
                self.watcher = Some(IfWatcher::new().await.expect("if-watch"));
            }
        }

        // a pending future or watcher
        let f1 = if let Some(watcher) = self.watcher.as_mut() {
            watcher.next().boxed()
        } else {
            future::pending().boxed()
        };

        let f2 = self.inner.accept().boxed();

        let either = future::select(f1, f2).await;
        match either {
            Either::Left((evt, _)) => {
                let evt = evt?;
                match evt {
                    IfEvent::Up(ip) => Ok(ListenerEvent::AddressAdded(ip_to_multiaddr(ip.addr(), self.port))),
                    IfEvent::Down(ip) => Ok(ListenerEvent::AddressDeleted(ip_to_multiaddr(ip.addr(), self.port))),
                }
            }
            Either::Right((r, _)) => {
                let (stream, sock_addr) = r?;
                apply_config(&self.config, &stream)?;

                let local_addr = stream.local_addr()?;
                let la = sock_to_multiaddr(local_addr);
                let ra = sock_to_multiaddr(sock_addr);
                Ok(ListenerEvent::Accepted(TcpTransStream { inner: stream, la, ra }))
            }
        }
    }

    fn multi_addr(&self) -> Option<&Multiaddr> {
        self.listen_address.as_ref()
    }
}

/// Wraps around a `TcpStream` and adds logging for important events.
#[derive(Debug)]
pub struct TcpTransStream {
    inner: TcpStream,
    la: Multiaddr,
    ra: Multiaddr,
}

impl ConnectionInfo for TcpTransStream {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl Drop for TcpTransStream {
    fn drop(&mut self) {
        if let Ok(addr) = self.inner.peer_addr() {
            log::debug!("Dropped TCP connection to {:?}", addr);
        } else {
            log::debug!("Dropped TCP connection to unterminated peer");
        }
    }
}

/// Applies the socket configuration parameters to a socket.
fn apply_config(config: &TcpConfig, socket: &TcpStream) -> Result<(), io::Error> {
    if let Some(ttl) = config.ttl {
        socket.set_ttl(ttl)?;
    }

    if let Some(nodelay) = config.nodelay {
        socket.set_nodelay(nodelay)?;
    }

    Ok(())
}

impl AsyncRead for TcpTransStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.inner), cx, buf)
    }
}

impl AsyncWrite for TcpTransStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.inner), cx)
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        //return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let mut addr = Multiaddr::from(ip);
    addr.push(Protocol::Tcp(port));
    addr
}

fn sock_to_multiaddr(sock: SocketAddr) -> Multiaddr {
    let mut addr = Multiaddr::from(sock.ip());
    addr.push(Protocol::Tcp(sock.port()));
    addr
}

// Collect all local host addresses and use the provided port number as listen port.
#[allow(dead_code)]
fn host_addresses(port: u16) -> io::Result<Vec<(IpAddr, IpNet, Multiaddr)>> {
    let mut addrs = Vec::new();
    for iface in get_if_addrs()? {
        let ip = iface.ip();
        let ma = ip_to_multiaddr(ip, port);
        let ipn = match iface.addr {
            IfAddr::V4(ip4) => {
                let prefix_len = (!u32::from_be_bytes(ip4.netmask.octets())).leading_zeros();
                let ipnet =
                    Ipv4Net::new(ip4.ip, prefix_len as u8).expect("prefix_len is the number of bits in a u32, so can not exceed 32");
                IpNet::V4(ipnet)
            }
            IfAddr::V6(ip6) => {
                let prefix_len = (!u128::from_be_bytes(ip6.netmask.octets())).leading_zeros();
                let ipnet =
                    Ipv6Net::new(ip6.ip, prefix_len as u8).expect("prefix_len is the number of bits in a u128, so can not exceed 128");
                IpNet::V6(ipnet)
            }
        };
        log::info!("adding host address {:}", ma);
        addrs.push((ip, ipn, ma))
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::multiaddr_to_socketaddr;
    use super::TcpConfig;
    use futures::{AsyncReadExt, AsyncWriteExt};
    use libp2prs_core::multiaddr::Multiaddr;
    use libp2prs_core::transport::ListenerEvent;
    use libp2prs_core::Transport;
    use libp2prs_runtime::task;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::Ipv6Addr;

        assert!(multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()).is_err());

        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/255.255.255.255/tcp/8080".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,)),
                8080,
            ))
        );
    }

    #[test]
    fn dialer_and_listener_timeout() {
        fn test1(addr: Multiaddr) {
            task::block_on(async move {
                let mut timeout_listener = TcpConfig::new().timeout(Duration::from_secs(1)).listen_on(addr).unwrap();
                assert!(timeout_listener.accept().await.is_err());
            });
        }

        fn test2(addr: Multiaddr) {
            task::block_on(async move {
                let mut tcp = TcpConfig::new().timeout(Duration::from_secs(1));
                assert!(tcp.dial(addr.clone()).await.is_err());
            });
        }

        test1("/ip4/127.0.0.1/tcp/1111".parse().unwrap());
        test2("/ip4/127.0.0.1/tcp/1111".parse().unwrap());
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        fn test(addr: Multiaddr) {
            let mut tcp_listener = TcpConfig::new().listen_on(addr).unwrap();
            let srv_addr = tcp_listener.multi_addr().cloned().unwrap();
            task::spawn(async move {
                loop {
                    let mut socket = match tcp_listener.accept().await.unwrap() {
                        ListenerEvent::Accepted(s) => s,
                        _ => continue,
                    };

                    let mut buf = [0u8; 3];
                    socket.read_exact(&mut buf).await.unwrap();
                    assert_eq!(buf, [1, 2, 3]);
                    socket.write_all(&[4, 5, 6]).await.unwrap();
                }
            });

            task::block_on(async move {
                let mut tcp = TcpConfig::new();

                // Obtain a future socket through dialing
                let mut socket = tcp.dial(srv_addr).await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/1110".parse().unwrap());
        test("/ip6/::1/tcp/1110".parse().unwrap());
    }
}
