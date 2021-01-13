// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::tls;
use libp2prs_core::transport::TransportError;
use libp2prs_core::Multiaddr;
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
