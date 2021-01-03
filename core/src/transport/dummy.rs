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

use crate::transport::{ConnectionInfo, IListener, ITransport, Transport, TransportError, TransportListener};
use crate::Multiaddr;
use async_trait::async_trait;
use std::fmt;

/// Implementation of `Transport` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
#[derive(Clone)]
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

#[async_trait]
impl Transport for DummyTransport {
    type Output = DummyStream;

    fn listen_on(&mut self, _addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        Err(TransportError::Internal)
    }

    async fn dial(&mut self, _addr: Multiaddr) -> Result<Self::Output, TransportError> {
        Err(TransportError::Internal)
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        vec![0]
    }
}

pub struct DummyListener;

#[async_trait]
impl TransportListener for DummyListener {
    type Output = DummyStream;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        Err(TransportError::Internal)
    }

    fn multi_addr(&self) -> Vec<Multiaddr> {
        vec![]
    }
}

pub struct DummyStream(());

impl ConnectionInfo for DummyStream {
    fn local_multiaddr(&self) -> Multiaddr {
        unimplemented!()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        unimplemented!()
    }
}
