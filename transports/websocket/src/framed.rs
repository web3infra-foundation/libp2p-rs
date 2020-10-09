use crate::connection::{Connection, TlsOrPlain};
use crate::{error::WsError, tls};
use async_trait::async_trait;
use either::Either;
use futures::prelude::*;
use libp2p_core::transport::ConnectionInfo;
use libp2p_core::transport::{IListener, ITransport};
use libp2p_core::{
    either::EitherOutput,
    multiaddr::{Multiaddr, Protocol},
    transport::{TransportError, TransportListener},
    Transport,
};
use libp2p_tcp::TcpTransStream;
use log::{debug, error, trace};
use soketto::{connection, extension::deflate::Deflate, handshake};
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: usize = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads which does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`crate::WsConfig`] if you require the latter.

#[derive(Debug,Clone)]
pub struct WsConfig {
    transport: ITransport<TcpTransStream>,
    pub(crate) inner_config: InnerConfig,
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
#[derive(Debug)]
pub struct WsTransListener {
    inner: IListener<TcpTransStream>,
    inner_config: InnerConfig,
    use_tls: bool,
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
    type Output = Connection<TcpTransStream>;
    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let raw_stream = self.inner.accept().await?;
        let inner_raw_stream = raw_stream.clone();
        let local_addr = raw_stream.local_multiaddr();
        let remote_addr = raw_stream.remote_multiaddr();
        let remote1 = remote_addr.clone(); // used for logging
        let remote2 = remote_addr.clone(); // used for logging
        let tls_config = self.inner_config.tls_config.clone();
        trace!("incoming connection from {}", remote1);
        let stream = if self.use_tls {
            // begin TLS session
            let server = tls_config.server.expect("for use_tls we checked server is not none");
            trace!("awaiting TLS handshake with {}", remote1);
            let stream = server
                .accept(raw_stream)
                .map_err(move |e| {
                    debug!("TLS handshake with {} failed: {}", remote1, e);
                    WsError::Tls(tls::Error::from(e))
                })
                .await?;

            let stream: TlsOrPlain<_> = EitherOutput::A(EitherOutput::B(stream));
            stream
        } else {
            // continue with plain stream
            EitherOutput::B(raw_stream)
        };

        trace!("receiving websocket handshake request from {}", remote2);
        let mut server = handshake::Server::new(stream);

        if self.inner_config.use_deflate {
            server.add_extension(Box::new(Deflate::new(connection::Mode::Server)));
        }

        let ws_key = {
            let request = server.receive_request().map_err(|e| WsError::Handshake(Box::new(e))).await?;
            request.into_key()
        };

        debug!("accepting websocket handshake request from {}", remote2);

        let response = handshake::server::Response::Accept {
            key: &ws_key,
            protocol: None,
        };

        server.send_response(&response).map_err(|e| WsError::Handshake(Box::new(e))).await?;

        let conn = {
            let mut builder = server.into_builder();
            builder.set_max_message_size(self.inner_config.max_data_size);
            builder.set_max_frame_size(self.inner_config.max_data_size);
            Connection::new(inner_raw_stream, builder, local_addr, remote_addr)
        };
        Ok(conn)
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}

#[async_trait]
impl Transport for WsConfig {
    type Output = Connection<TcpTransStream>;
    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
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
                        return Err(TransportError::Internal);
                    }
                    remaining_redirects -= 1;
                    addr = location_to_multiaddr(&redirect)?;
                }
                Ok(Either::Right(conn)) => return Ok(conn),
                Err(e) => {
                    error!("websocket transport dial error:{}", e);
                    return Err(e.into());
                }
            }
        }
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }
}

impl WsConfig {
    /// Attempty to dial the given address and perform a websocket handshake.
    async fn dial_once(&mut self, address: Multiaddr) -> Result<Either<String, Connection<TcpTransStream>>, WsError> {
        trace!("dial address: {}", address);
        let (host_port, dns_name) = host_and_dnsname(&address)?;
        if dns_name.is_some() {
            trace!("host_port: {:?}  dns_name:{:?}", host_port, dns_name.clone().unwrap());
        }
        let mut inner_addr = address.clone();

        let (use_tls, path) = match inner_addr.pop() {
            Some(Protocol::Ws(path)) => (false, path),
            Some(Protocol::Wss(path)) => {
                if dns_name.is_none() {
                    debug!("no DNS name in {}", address);
                    return Err(WsError::InvalidMultiaddr(address));
                }
                (true, path)
            }
            _ => {
                debug!("{} is not a websocket multiaddr", address);
                return Err(WsError::InvalidMultiaddr(address));
            }
        };

        let raw_stream = self
            .transport
            .dial(inner_addr)
            .map_err(|e| match e {
                TransportError::MultiaddrNotSupported(a) => WsError::InvalidMultiaddr(a),
                _ => WsError::Transport(e),
            })
            .await?;
        let inner_raw_stream = raw_stream.clone();
        trace!("connected to {}", address);
        let local_addr = raw_stream.local_multiaddr();
        let remote_addr = raw_stream.remote_multiaddr();
        let stream = if use_tls {
            // begin TLS session
            let dns_name = dns_name.expect("for use_tls we have checked that dns_name is some");
            trace!("starting TLS handshake with {}", address);
            let stream = self
                .inner_config
                .tls_config
                .client
                .connect(&dns_name, raw_stream)
                .map_err(|e| {
                    debug!("TLS handshake with {} failed: {}", address, e);
                    WsError::Tls(tls::Error::from(e))
                })
                .await?;

            let stream: TlsOrPlain<_> = EitherOutput::A(EitherOutput::A(stream));
            stream
        } else {
            // continue with plain stream
            EitherOutput::B(raw_stream)
        };

        trace!("sending websocket handshake request to {}", address);

        let mut client = handshake::Client::new(stream, &host_port, path.as_ref());

        if self.inner_config.use_deflate {
            client.add_extension(Box::new(Deflate::new(connection::Mode::Client)));
        }

        match client.handshake().map_err(|e| WsError::Handshake(Box::new(e))).await? {
            handshake::ServerResponse::Redirect { status_code, location } => {
                debug!("received redirect ({}); location: {}", status_code, location);
                Ok(Either::Left(location))
            }
            handshake::ServerResponse::Rejected { status_code } => {
                let msg = format!("server rejected handshake; status code = {}", status_code);
                Err(WsError::Handshake(msg.into()))
            }
            handshake::ServerResponse::Accepted { .. } => {
                debug!("websocket handshake with {} successful", address);
                Ok(Either::Right(Connection::new(
                    inner_raw_stream,
                    client.into_builder(),
                    local_addr,
                    remote_addr,
                )))
            }
        }
    }
}

impl From<WsError> for TransportError {
    fn from(e: WsError) -> Self {
        match e {
            WsError::InvalidMultiaddr(a) => TransportError::MultiaddrNotSupported(a),
            _ => TransportError::Internal,
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
