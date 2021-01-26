// Copyright 2017-2019 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for Websockets.

pub mod connection;
pub mod error;
pub mod framed;
pub mod tls;

use crate::connection::TlsOrPlain;
use async_trait::async_trait;
use libp2prs_core::transport::{IListener, ITransport};
use libp2prs_core::{multiaddr::Multiaddr, transport::TransportError, Transport};
use libp2prs_dns::DnsConfig;
use libp2prs_tcp::{TcpConfig, TcpTransStream};

/// A Websocket transport.
#[derive(Clone)]
pub struct WsConfig {
    inner: framed::WsConfig,
}

impl WsConfig {
    /// Create a new websocket transport based on the tcp transport.
    pub fn new() -> Self {
        framed::WsConfig::new(TcpConfig::default().box_clone()).into()
    }

    /// Create a new websocket transport based on the dns transport.
    pub fn new_with_dns() -> Self {
        framed::WsConfig::new(DnsConfig::new(TcpConfig::default()).box_clone()).into()
    }

    /// Return the configured maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        self.inner.inner_config.max_redirects()
    }

    /// Set max. number of redirects to follow.
    pub fn set_max_redirects(&mut self, max: u8) -> &mut Self {
        self.inner.inner_config.set_max_redirects(max);
        self
    }

    /// Get the max. frame data size we support.
    pub fn max_data_size(&self) -> usize {
        self.inner.inner_config.max_data_size()
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: usize) -> &mut Self {
        self.inner.inner_config.set_max_data_size(size);
        self
    }

    /// Set the TLS configuration if TLS support is desired.
    pub fn set_tls_config(&mut self, c: tls::Config) -> &mut Self {
        self.inner.inner_config.set_tls_config(c);
        self
    }

    /// Should the deflate extension (RFC 7692) be used if supported?
    pub fn use_deflate(&mut self, flag: bool) -> &mut Self {
        self.inner.inner_config.use_deflate(flag);
        self
    }
}

impl Default for WsConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<framed::WsConfig> for WsConfig {
    fn from(framed: framed::WsConfig) -> Self {
        WsConfig { inner: framed }
    }
}

#[async_trait]
impl Transport for WsConfig {
    type Output = connection::Connection<TlsOrPlain<TcpTransStream>>;
    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        self.inner.listen_on(addr)
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        self.inner.dial(addr).await
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        self.inner.protocols()
    }
}

#[cfg(test)]
mod tests {
    use super::WsConfig;
    use libp2prs_core::transport::ListenerEvent;
    use libp2prs_core::Multiaddr;
    use libp2prs_core::Transport;
    use libp2prs_runtime::task;
    use libp2prs_traits::{ReadEx, WriteEx};

    #[test]
    fn dialer_connects_to_listener_ipv4() {
        let listen_addr = "/ip4/127.0.0.1/tcp/38099/ws".parse().unwrap();
        let dial_addr = "/ip4/127.0.0.1/tcp/38099/ws".parse().unwrap();
        let s = task::spawn(async { server(listen_addr).await });
        let c = task::spawn(async { client(dial_addr, false).await });
        task::block_on(async {
            assert_eq!(futures::join!(s, c), (Some(true), Some(true)));
        });
    }

    #[test]
    fn dialer_connects_to_listener_dns() {
        let listen_addr = "/ip4/127.0.0.1/tcp/38100/ws".parse().unwrap();
        let dial_addr = "/dns4/localhost/tcp/38100/ws".parse().unwrap();
        let s = task::spawn(async { server(listen_addr).await });
        let c = task::spawn(async { client(dial_addr, true).await });
        task::block_on(async {
            assert_eq!(futures::join!(s, c), (Some(true), Some(true)));
        });
    }

    #[test]
    fn dialer_connects_to_listener_ipv6() {
        let listen_addr = "/ip6/::1/tcp/38101/ws".parse().unwrap();
        let dial_addr = "/ip6/::1/tcp/38101/ws".parse().unwrap();
        let s = task::spawn(async { server(listen_addr).await });
        let c = task::spawn(async { client(dial_addr, false).await });
        task::block_on(async {
            assert_eq!(futures::join!(s, c), (Some(true), Some(true)));
        });
    }

    async fn server(listen_addr: Multiaddr) -> bool {
        let ws_config: WsConfig = WsConfig::new();
        let mut listener = ws_config
            .clone()
            .timeout(std::time::Duration::from_secs(5))
            .listen_on(listen_addr.clone())
            .expect("listener");

        let mut stream = match listener.accept().await.expect("no error") {
            ListenerEvent::Accepted(s) => s,
            _ => panic!("unreachable"),
        };
        let mut buf = vec![0_u8; 3];
        stream.read_exact2(&mut buf).await.expect("read_exact");
        vec![1, 23, 5] == buf
    }

    async fn client(dial_addr: Multiaddr, dns: bool) -> bool {
        let ws_config: WsConfig;
        if dns {
            ws_config = WsConfig::new_with_dns();
        } else {
            ws_config = WsConfig::new();
        }
        task::sleep(std::time::Duration::from_millis(200)).await;
        let conn = ws_config.timeout(std::time::Duration::from_secs(5)).dial(dial_addr.clone()).await;
        let mut conn = conn.expect("");
        let data = vec![1_u8, 23, 5];
        log::debug!("[Client] write data {:?}", data);
        let r = conn.write_all2(&data).await;
        r.is_ok()
    }
}
