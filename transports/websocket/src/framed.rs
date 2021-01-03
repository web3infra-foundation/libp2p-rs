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

// use crate::connection::{Connection, TlsOrPlain};
use crate::connection::{Connection, TlsClientStream, TlsOrPlain, TlsServerStream};
use crate::{error::WsError, tls};
use async_trait::async_trait;
use either::Either;
use futures::prelude::*;
use libp2prs_core::transport::ConnectionInfo;
use libp2prs_core::transport::{IListener, ITransport};
use libp2prs_core::{
    either::AsyncEitherOutput,
    multiaddr::{protocol, protocol::Protocol, Multiaddr},
    transport::{TransportError, TransportListener},
    Transport,
};
use libp2prs_tcp::TcpTransStream;
use log::{debug, info, trace};
use soketto::{connection, extension::deflate::Deflate, handshake};
use std::fmt;
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: usize = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads which does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`crate::WsConfig`] if you require the latter.

#[derive(Clone)]
pub struct WsConfig {
    transport: ITransport<TcpTransStream>,
    pub(crate) inner_config: InnerConfig,
}

impl fmt::Debug for WsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WsConfig").field("Config", &self.inner_config).finish()
    }
}

impl WsConfig {
    /// Create a new websocket transport based on another transport.
    pub fn new(transport: ITransport<TcpTransStream>) -> Self {
        WsConfig {
            transport,
            inner_config: InnerConfig::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InnerConfig {
    max_data_size: usize,
    tls_config: tls::Config,
    max_redirects: u8,
    use_deflate: bool,
}

impl InnerConfig {
    /// Create a new websocket transport based on another transport.
    pub fn new() -> Self {
        InnerConfig {
            max_data_size: MAX_DATA_SIZE,
            tls_config: tls::Config::client(),
            max_redirects: 0,
            use_deflate: false,
        }
    }

    /// Return the configured maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        self.max_redirects
    }

    /// Set max. number of redirects to follow.
    pub fn set_max_redirects(&mut self, max: u8) -> &mut Self {
        self.max_redirects = max;
        self
    }

    /// Get the max. frame data size we support.
    pub fn max_data_size(&self) -> usize {
        self.max_data_size
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: usize) -> &mut Self {
        self.max_data_size = size;
        self
    }

    /// Set the TLS configuration if TLS support is desired.
    pub fn set_tls_config(&mut self, c: tls::Config) -> &mut Self {
        self.tls_config = c;
        self
    }

    /// Should the deflate extension (RFC 7692) be used if supported?
    pub fn use_deflate(&mut self, flag: bool) -> &mut Self {
        self.use_deflate = flag;
        self
    }
}

pub struct WsTransListener {
    inner: IListener<TcpTransStream>,
    inner_config: InnerConfig,
    use_tls: bool,
}

impl fmt::Debug for WsTransListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WsTransListener")
            .field("Config", &self.inner_config)
            .field("tls", &self.use_tls)
            .finish()
    }
}

impl WsTransListener {
    pub(crate) fn new(inner: IListener<TcpTransStream>, inner_config: InnerConfig, use_tls: bool) -> Self {
        Self {
            inner,
            inner_config,
            use_tls,
        }
    }
}

#[async_trait]
impl TransportListener for WsTransListener {
    type Output = Connection<TlsOrPlain<TcpTransStream>>;
    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let raw_stream = self.inner.accept().await?;
        let local_addr = raw_stream.local_multiaddr();
        let remote_addr = raw_stream.remote_multiaddr();
        let remote1 = remote_addr.clone(); // used for logging
        let remote2 = remote_addr.clone(); // used for logging
        let tls_config = self.inner_config.tls_config.clone();
        trace!("[Server] incoming connection from {}", remote1);
        let stream = if self.use_tls {
            // begin TLS session
            let server = tls_config.server.expect("for use_tls we checked server is not none");
            trace!("[Server] awaiting TLS handshake with {}", remote1);
            let stream = server.accept(raw_stream).await.map_err(move |e| {
                debug!("[Server] TLS handshake with {} failed: {}", remote1, e);
                WsError::Tls(tls::Error::from(e))
            })?;

            let stream = TlsServerStream(stream);

            let stream: TlsOrPlain<_> = AsyncEitherOutput::A(AsyncEitherOutput::B(stream));
            stream
        } else {
            // continue with plain stream
            AsyncEitherOutput::B(raw_stream)
        };

        trace!("[Server] receiving websocket handshake request from {}", remote2);
        let mut server = handshake::Server::new(stream);

        if self.inner_config.use_deflate {
            server.add_extension(Box::new(Deflate::new(connection::Mode::Server)));
        }

        let ws_key = {
            let request = server.receive_request().await.map_err(|e| WsError::Handshake(Box::new(e)))?;
            request.into_key()
        };

        debug!("[Server] accepting websocket handshake request from {}", remote2);

        let response = handshake::server::Response::Accept {
            key: &ws_key,
            protocol: None,
        };

        server.send_response(&response).await.map_err(|e| WsError::Handshake(Box::new(e)))?;

        let conn = {
            let mut builder = server.into_builder();
            builder.set_max_message_size(self.inner_config.max_data_size);
            builder.set_max_frame_size(self.inner_config.max_data_size);
            Connection::new(builder, local_addr, remote_addr)
        };
        Ok(conn)
    }

    fn multi_addr(&self) -> Vec<Multiaddr> {
        self.inner.multi_addr()
    }
}

#[async_trait]
impl Transport for WsConfig {
    type Output = Connection<TlsOrPlain<TcpTransStream>>;
    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        log::debug!("WebSocket listen on addr: {}", addr);
        let mut inner_addr = addr.clone();

        let (use_tls, _proto) = match inner_addr.pop() {
            Some(p @ Protocol::Wss(_)) => {
                if self.inner_config.tls_config.server.is_some() {
                    (true, p)
                } else {
                    debug!("/wss address but TLS server support is not configured");
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
            }
            Some(p @ Protocol::Ws(_)) => (false, p),
            _ => {
                debug!("{} is not a websocket multiaddr", addr);
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
        };
        let inner_listener = self.transport.listen_on(addr)?;
        let listener = WsTransListener::new(inner_listener, self.inner_config.clone(), use_tls);
        Ok(Box::new(listener))
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        // Quick sanity check of the provided Multiaddr.
        if let Some(Protocol::Ws(_)) | Some(Protocol::Wss(_)) = addr.iter().last() {
            // ok
        } else {
            debug!("{} is not a websocket multiaddr", addr);
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        // We are looping here in order to follow redirects (if any):
        let mut remaining_redirects = self.inner_config.max_redirects;
        let mut addr = addr;
        loop {
            match self.dial_once(addr).await {
                Ok(Either::Left(redirect)) => {
                    if remaining_redirects == 0 {
                        debug!("too many redirects");
                        return Err(WsError::TooManyRedirects.into());
                    }
                    remaining_redirects -= 1;
                    addr = location_to_multiaddr(&redirect)?;
                }
                Ok(Either::Right(conn)) => return Ok(conn),
                Err(e) => {
                    debug!("websocket transport dial error:{}", e);
                    return Err(e.into());
                }
            }
        }
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        vec![protocol::WS, protocol::WSS]
    }
}

impl WsConfig {
    /// Attempty to dial the given address and perform a websocket handshake.
    async fn dial_once(&mut self, address: Multiaddr) -> Result<Either<String, Connection<TlsOrPlain<TcpTransStream>>>, WsError> {
        trace!("[Client] dial address: {}", address);
        let (host_port, dns_name) = host_and_dnsname(&address)?;
        if dns_name.is_some() {
            trace!("[Client] host_port: {:?}  dns_name:{:?}", host_port, dns_name.clone().unwrap());
        }
        let mut inner_addr = address.clone();

        let (use_tls, path) = match inner_addr.pop() {
            Some(Protocol::Ws(path)) => (false, path),
            Some(Protocol::Wss(path)) => {
                if dns_name.is_none() {
                    debug!("[Client] no DNS name in {}", address);
                    return Err(WsError::InvalidMultiaddr(address));
                }
                (true, path)
            }
            _ => {
                debug!("[Client] {} is not a websocket multiaddr", address);
                return Err(WsError::InvalidMultiaddr(address));
            }
        };

        let raw_stream = self.transport.dial(inner_addr).await.map_err(WsError::Transport)?;
        trace!("[Client] connected to {}", address);
        let local_addr = raw_stream.local_multiaddr();
        let remote_addr = raw_stream.remote_multiaddr();
        let stream = if use_tls {
            // begin TLS session
            let dns_name = dns_name.expect("for use_tls we have checked that dns_name is some");
            trace!("[Client] starting TLS handshake with {}", address);
            let stream = self
                .inner_config
                .tls_config
                .client
                .connect(&dns_name, raw_stream)
                .await
                .map_err(|e| {
                    debug!("[Client] TLS handshake with {} failed: {}", address, e);
                    WsError::Tls(tls::Error::from(e))
                })?;

            let stream = TlsClientStream(stream);

            let stream: TlsOrPlain<_> = AsyncEitherOutput::A(AsyncEitherOutput::A(stream));
            stream
        } else {
            // continue with plain stream
            AsyncEitherOutput::B(raw_stream)
        };

        trace!("[Client] sending websocket handshake request to {}", address);

        let mut client = handshake::Client::new(stream, &host_port, path.as_ref());

        if self.inner_config.use_deflate {
            client.add_extension(Box::new(Deflate::new(connection::Mode::Client)));
        }

        match client
            .handshake()
            .map_err(|e| {
                info!("[Client] {:?}", e);
                WsError::Handshake(Box::new(e))
            })
            .await?
        {
            handshake::ServerResponse::Redirect { status_code, location } => {
                debug!("[Client] received redirect ({}); location: {}", status_code, location);
                Ok(Either::Left(location))
            }
            handshake::ServerResponse::Rejected { status_code } => {
                let msg = format!("[Client] server rejected handshake; status code = {}", status_code);
                Err(WsError::Handshake(msg.into()))
            }
            handshake::ServerResponse::Accepted { .. } => {
                debug!("[Client] websocket handshake with {} successful", address);
                Ok(Either::Right(Connection::new(client.into_builder(), local_addr, remote_addr)))
            }
        }
    }
}

impl From<WsError> for TransportError {
    fn from(e: WsError) -> Self {
        match e {
            WsError::InvalidMultiaddr(a) => TransportError::MultiaddrNotSupported(a),
            _ => TransportError::WsError(Box::new(e)),
        }
    }
}

// Extract host, port and optionally the DNS name from the given [`Multiaddr`].
fn host_and_dnsname(addr: &Multiaddr) -> Result<(String, Option<webpki::DNSName>), WsError> {
    let mut iter = addr.iter();
    match (iter.next(), iter.next()) {
        (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) => Ok((format!("{}:{}", ip, port), None)),
        (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) => Ok((format!("{}:{}", ip, port), None)),
        (Some(Protocol::Dns(h)), Some(Protocol::Tcp(port))) => {
            Ok((format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?.to_owned())))
        }
        (Some(Protocol::Dns4(h)), Some(Protocol::Tcp(port))) => {
            Ok((format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?.to_owned())))
        }
        (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port))) => {
            Ok((format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?.to_owned())))
        }
        _ => {
            debug!("multi-address format not supported: {}", addr);
            Err(WsError::InvalidMultiaddr(addr.clone()))
        }
    }
}

// Given a location URL, build a new websocket [`Multiaddr`].
fn location_to_multiaddr(location: &str) -> Result<Multiaddr, WsError> {
    match Url::parse(location) {
        Ok(url) => {
            let mut a = Multiaddr::empty();
            match url.host() {
                Some(url::Host::Domain(h)) => a.push(Protocol::Dns(h.into())),
                Some(url::Host::Ipv4(ip)) => a.push(Protocol::Ip4(ip)),
                Some(url::Host::Ipv6(ip)) => a.push(Protocol::Ip6(ip)),
                None => return Err(WsError::InvalidRedirectLocation),
            }
            if let Some(p) = url.port() {
                a.push(Protocol::Tcp(p))
            }
            let s = url.scheme();
            if s.eq_ignore_ascii_case("https") | s.eq_ignore_ascii_case("wss") {
                a.push(Protocol::Wss(url.path().into()))
            } else if s.eq_ignore_ascii_case("http") | s.eq_ignore_ascii_case("ws") {
                a.push(Protocol::Ws(url.path().into()))
            } else {
                debug!("unsupported scheme: {}", s);
                return Err(WsError::InvalidRedirectLocation);
            }
            Ok(a)
        }
        Err(e) => {
            debug!("failed to parse url as multi-address: {:?}", e);
            Err(WsError::InvalidRedirectLocation)
        }
    }
}
