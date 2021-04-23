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

use crate::identity::Keypair;
use crate::muxing::{IReadWrite, IStreamMuxer, StreamMuxer, StreamMuxerEx};
use crate::secure_io::SecureInfo;
use crate::transport::{ConnectionInfo, TransportError};
use crate::upgrade::{UpgradeInfo, Upgrader};
use crate::{Multiaddr, PeerId, PublicKey};
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::task::{Context, Poll};
use futures::{AsyncRead, AsyncWrite};
use log::trace;
use pin_project::__private::Pin;
use std::{fmt, io};

/// Implementation of dummy `Upgrader` that doesn't do anything practice.
///
/// Useful for testing purposes.
pub struct DummyUpgrader;

#[pin_project::pin_project]
pub struct DummyStream<T>(#[pin] pub(crate) T);

impl<T> Clone for DummyStream<T> {
    fn clone(&self) -> Self {
        unimplemented!()
    }
}

impl<T> fmt::Debug for DummyStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DummyStream")
    }
}

impl DummyUpgrader {
    /// Builds a new `DummyUpgrader`.
    pub fn new() -> Self {
        DummyUpgrader
    }
}

impl Default for DummyUpgrader {
    fn default() -> Self {
        DummyUpgrader::new()
    }
}

impl fmt::Debug for DummyUpgrader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DummyUpgrader")
    }
}

impl Clone for DummyUpgrader {
    fn clone(&self) -> Self {
        DummyUpgrader
    }
}

impl UpgradeInfo for DummyUpgrader {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/dummy/1.0.0"]
    }
}

#[async_trait]
impl<T: Send + 'static> Upgrader<T> for DummyUpgrader {
    type Output = DummyStream<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("dummy upgrader, upgrade inbound connection");
        Ok(DummyStream(socket))
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("dummy upgrader, upgrade outbound connection");
        Ok(DummyStream(socket))
    }
}

impl<T: AsyncRead + Send> AsyncRead for DummyStream<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(self.project().0, cx, buf)
    }
}

impl<T: AsyncWrite + Send> AsyncWrite for DummyStream<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(self.project().0, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(self.project().0, cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_close(self.project().0, cx)
    }
}

#[async_trait]
impl<T: ConnectionInfo> StreamMuxer for DummyStream<T> {
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError> {
        Err(TransportError::Internal)
    }

    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError> {
        Err(TransportError::Internal)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        None
    }

    fn box_clone(&self) -> IStreamMuxer {
        unimplemented!()
    }
}

impl<T: ConnectionInfo> ConnectionInfo for DummyStream<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.0.local_multiaddr()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.0.remote_multiaddr()
    }
}

/// A fake implementation of SecureInfo for DummyStream<T>
/// required by StreamMuxer: SecureInfo + ...
impl<T> SecureInfo for DummyStream<T> {
    fn local_peer(&self) -> PeerId {
        PeerId::random()
    }

    fn remote_peer(&self) -> PeerId {
        PeerId::random()
    }

    fn local_priv_key(&self) -> Keypair {
        Keypair::generate_ed25519()
    }

    fn remote_pub_key(&self) -> PublicKey {
        Keypair::generate_ed25519().public()
    }
}

impl<T: ConnectionInfo> StreamMuxerEx for DummyStream<T> {}
