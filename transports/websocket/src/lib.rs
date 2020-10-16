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

use async_trait::async_trait;
use libp2p_core::transport::{IListener, ITransport};
use libp2p_core::{multiaddr::Multiaddr, transport::TransportError, Transport};
use libp2p_dns::DnsConfig;
use libp2p_tcp::{TcpConfig, TcpTransStream};

/// A Websocket transport.
#[derive(Clone, Debug)]
pub struct WsConfig {
    inner: framed::WsConfig,
}

impl WsConfig {
    /// Create a new websocket transport based on the given transport.
    pub fn new() -> Self {
        framed::WsConfig::new(TcpConfig::default().box_clone()).into()
    }

    /// Create a new websocket transport based on the given transport.
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
    type Output = connection::Connection<TcpTransStream>;
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
    use super::connection::IncomingData;
    use super::WsConfig;
    use async_std::task;
    use futures::prelude::*;
    use libp2p_core::Multiaddr;
    use libp2p_core::Transport;
    #[test]
    fn dialer_connects_to_listener_ipv4() {
        env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        let listen_addr = "/ip4/127.0.0.1/tcp/38099/ws".parse().unwrap();
        let dial_addr = "/ip4/127.0.0.1/tcp/38099/ws".parse().unwrap();
        let s = task::spawn(async {
            server(listen_addr).await;
        });
        let c = task::spawn(async {
            client(dial_addr, false).await;
        });
        futures::executor::block_on(async {
            futures::join!(s, c);
        });
    }

    #[test]
    fn dialer_connects_to_listener_dns() {
        env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        let listen_addr = "/ip4/127.0.0.1/tcp/38099/ws".parse().unwrap();
        let dial_addr = "/dns4/localhost/tcp/38099/ws".parse().unwrap();
        let s = task::spawn(async {
            server(listen_addr).await;
        });
        let c = task::spawn(async {
            client(dial_addr, true).await;
        });
        futures::executor::block_on(async {
            futures::join!(s, c);
        });
    }

    #[test]
    fn dialer_connects_to_listener_ipv6() {
        env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        let listen_addr = "/ip6/::1/tcp/38088/ws".parse().unwrap();
        let dial_addr = "/ip6/::1/tcp/38088/ws".parse().unwrap();
        let s = task::spawn(async {
            server(listen_addr).await;
        });
        let c = task::spawn(async {
            client(dial_addr, false).await;
        });
        futures::executor::block_on(async {
            futures::join!(s, c);
        });
    }

    async fn server(listen_addr: Multiaddr) {
        let ws_config: WsConfig = WsConfig::new();
        let mut listener = ws_config.clone().listen_on(listen_addr.clone()).expect("listener");
        let mut stream = listener.accept().await.expect("no error");
        while let Some(data) = stream.next().await {
            assert_eq!(IncomingData::Binary(vec![1, 23, 5]), data.unwrap());
        }
    }

    async fn client(dial_addr: Multiaddr, dns: bool) {
        let ws_config: WsConfig;
        if dns {
            ws_config = WsConfig::new_with_dns();
        } else {
            ws_config = WsConfig::new();
        }
        let conn = ws_config
            .outbound_timeout(std::time::Duration::new(5, 0))
            .dial(dial_addr.clone())
            .await;
        assert_eq!(true, conn.is_ok());
        let r = conn.unwrap().send_data(vec![1, 23, 5]).await;
        assert_eq!(true, r.is_ok());
    }
}
