use crate::transport::{Transport, TransportError, TransportListener};
use crate::Multiaddr;
use async_trait::async_trait;
use std::fmt;

/// Implementation of `Transport` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
pub struct DummyTransport;

impl DummyTransport {
    /// Builds a new `DummyTransport`.
    pub fn new() -> Self {
        DummyTransport
    }
}

impl Default for DummyTransport {
    fn default() -> Self {
        DummyTransport::new()
    }
}

impl fmt::Debug for DummyTransport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DummyTransport")
    }
}

impl Clone for DummyTransport {
    fn clone(&self) -> Self {
        DummyTransport
    }
}

#[async_trait]
impl Transport for DummyTransport {
    type Output = DummyStream;
    type Listener = DummyListener;

    fn listen_on(self, _addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        Err(TransportError::Internal)
    }

    async fn dial(self, _addr: Multiaddr) -> Result<Self::Output, TransportError> {
        Err(TransportError::Internal)
    }
}

pub struct DummyListener;

#[async_trait]
impl TransportListener for DummyListener {
    type Output = DummyStream;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        Err(TransportError::Internal)
    }

    fn multi_addr(&self) -> Multiaddr {
        "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()
    }
}

pub struct DummyStream(());
