use crate::tls;
use libp2p_core::transport::TransportError;
use libp2p_core::Multiaddr;
use std::{error, fmt};

/// Error in WebSockets.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum WsError {
    /// Error in the transport layer underneath.
    Transport(TransportError),
    /// A TLS related error.
    Tls(tls::Error),
    /// Websocket handshake error.
    Handshake(Box<dyn error::Error + Send + Sync>),
    /// The configured maximum of redirects have been made.
    TooManyRedirects,
    /// A multi-address is not supported.
    InvalidMultiaddr(Multiaddr),
    /// The location header URL was invalid.
    InvalidRedirectLocation,
    /// Websocket base framing error.
    Base(Box<dyn error::Error + Send + Sync>),
}

impl fmt::Display for WsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::Transport(err) => write!(f, "{}", err),
            WsError::Tls(err) => write!(f, "{}", err),
            WsError::Handshake(err) => write!(f, "{}", err),
            WsError::InvalidMultiaddr(ma) => write!(f, "invalid multi-address: {}", ma),
            WsError::TooManyRedirects => f.write_str("too many redirects"),
            WsError::InvalidRedirectLocation => f.write_str("invalid redirect location"),
            WsError::Base(err) => write!(f, "{}", err),
        }
    }
}

impl error::Error for WsError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            WsError::Transport(err) => Some(err),
            WsError::Tls(err) => Some(err),
            WsError::Handshake(err) => Some(&**err),
            WsError::Base(err) => Some(&**err),
            WsError::InvalidMultiaddr(_) | WsError::TooManyRedirects | WsError::InvalidRedirectLocation => None,
        }
    }
}

impl From<tls::Error> for WsError {
    fn from(e: tls::Error) -> Self {
        WsError::Tls(e)
    }
}
