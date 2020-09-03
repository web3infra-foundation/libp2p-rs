// Copyright 2019 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use crate::transport::{Transport, TransportError, TransportListener};
use crate::Multiaddr;
use std::{fmt, io, pin::Pin};
use futures::{prelude::*, task::Context, task::Poll};

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
    type Error = io::Error;
    type Listener = DummyListener;
    type ListenerUpgrade = futures::future::Pending<Result<Self::Output, io::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }
}

pub struct DummyListener;

#[async_trait]
impl TransportListener for DummyListener {
    type Output = ();
    type Error = io::Error;

    async fn accept(&mut self) -> Result<Self::Output, TransportError<Self::Error>> {
        Err(TransportError::Other(io::ErrorKind::Other.into()))
    }

    fn multi_addr(&self) -> Multiaddr {
        "/ip4/127.0.0.1/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap()
    }
}

/// Implementation of `AsyncRead` and `AsyncWrite`. Not meant to be instanciated.
pub struct DummyStream(());

impl fmt::Debug for DummyStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DummyStream")
    }
}

impl AsyncRead for DummyStream {
    fn poll_read(self: Pin<&mut Self>, _: &mut Context, _: &mut [u8])
        -> Poll<Result<usize, io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}

impl AsyncWrite for DummyStream {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context, _: &[u8])
        -> Poll<Result<usize, io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}
