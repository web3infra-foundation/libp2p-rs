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

mod error;
mod handshake;
// mod secure_stream;

use crate::error::PlaintextError;
use crate::handshake::handshake_plaintext::Remote;
// use crate::secure_stream::{SecureStream, SecureStreamReader, SecureStreamWriter};
use futures::{AsyncRead, AsyncWrite};
use libp2prs_core::identity::Keypair;
use libp2prs_core::secure_io::SecureInfo;
use libp2prs_core::transport::{ConnectionInfo, TransportError};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use libp2prs_core::{Multiaddr, PeerId, PublicKey};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;

pub mod structs_proto {
    include!(concat!(env!("OUT_DIR"), "/structs_proto.rs"));
}

const MAX_FRAME_SIZE: usize = 1024 * 1024 * 8;

/// Config for PlainText
#[derive(Clone)]
pub struct PlainTextConfig {
    pub(crate) key: Keypair,
    pub(crate) max_frame_length: usize,
}

impl PlainTextConfig {
    /// Create new config
    pub fn new(key: Keypair) -> Self {
        PlainTextConfig {
            key,
            max_frame_length: MAX_FRAME_SIZE,
        }
    }

    /// Attempts to perform a handshake on the given socket.
    ///
    /// On success, produces a `SecureStream` that can then be used to encode/decode
    /// communications, plus the remote info that contains public key and peer_id
    pub async fn handshake<T>(self, socket: T) -> Result<(T, Remote), PlaintextError>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        handshake::handshake_plaintext::handshake(socket, self).await
    }
}

impl UpgradeInfo for PlainTextConfig {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/plaintext/1.0.0"]
    }
}

#[async_trait]
impl<T> Upgrader<T> for PlainTextConfig
where
    T: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = PlainTextOutput<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }
}

async fn make_secure_output<T>(config: PlainTextConfig, socket: T) -> Result<PlainTextOutput<T>, TransportError>
where
    T: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let pri_key = config.key.clone();
    let la = socket.local_multiaddr();
    let ra = socket.remote_multiaddr();
    let (secure_stream, remote) = config.handshake(socket).await?;
    let output = PlainTextOutput {
        stream: secure_stream,
        local_priv_key: pri_key.clone(),
        local_peer_id: pri_key.public().into_peer_id(),
        remote_pub_key: remote.public_key,
        remote_peer_id: remote.peer_id,
        la,
        ra,
    };
    Ok(output)
}

/// Output of the plaintext protocol. It implements the SecureStream trait
#[pin_project::pin_project]
pub struct PlainTextOutput<T> {
    /// The encrypted stream, actually not any encrypt action.
    #[pin]
    pub stream: T,
    /// The private key of the local
    pub local_priv_key: Keypair,
    /// For convenience, the local peer ID, generated from local pub key
    pub local_peer_id: PeerId,
    /// The public key of the remote.
    pub remote_pub_key: PublicKey,
    /// For convenience, put a PeerId here, which is actually calculated from remote_key
    pub remote_peer_id: PeerId,
    /// The local multiaddr of this connection
    la: Multiaddr,
    /// The remote multiaddr of this connection
    ra: Multiaddr,
}

impl<T> SecureInfo for PlainTextOutput<T> {
    fn local_peer(&self) -> PeerId {
        self.local_peer_id
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_peer_id
    }

    fn local_priv_key(&self) -> Keypair {
        self.local_priv_key.clone()
    }

    fn remote_pub_key(&self) -> PublicKey {
        self.remote_pub_key.clone()
    }
}

impl<T: Send> ConnectionInfo for PlainTextOutput<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }
    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncRead for PlainTextOutput<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.stream.poll_read(cx, buf)
    }
}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncWrite for PlainTextOutput<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.stream.poll_close(cx)
    }
}

impl From<PlaintextError> for TransportError {
    fn from(e: PlaintextError) -> Self {
        TransportError::SecurityError(Box::new(e))
    }
}
