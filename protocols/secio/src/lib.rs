// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Aes Encrypted communication and handshake process implementation

#![deny(missing_docs)]

use async_trait::async_trait;

use crate::{crypto::cipher::CipherType, error::SecioError, exchange::KeyAgreement, handshake::procedure::handshake};

use libp2prs_core::identity::Keypair;
use libp2prs_core::{Multiaddr, PeerId, PublicKey};

use crate::codec::secure_stream::SecureStream;
use futures::{AsyncRead, AsyncWrite};
use libp2prs_core::secure_io::SecureInfo;
use libp2prs_core::transport::{ConnectionInfo, TransportError};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

/// Encrypted and decrypted codec implementation, and stream handle
pub mod codec;
/// Symmetric ciphers algorithms
pub mod crypto;
/// Error type
pub mod error;
/// Exchange information during the handshake
mod exchange;
/// Implementation of the handshake process
pub mod handshake;
/// Supported algorithms
mod support;

mod handshake_proto {
    include!(concat!(env!("OUT_DIR"), "/handshake_proto.rs"));
}

/// Public key generated temporarily during the handshake
pub type EphemeralPublicKey = Vec<u8>;

/// Possible digest algorithms.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Digest {
    /// Sha256 digest
    Sha256,
    /// Sha512 digest
    Sha512,
}

impl Digest {
    /// Returns the size in bytes of a digest of this kind.
    #[inline]
    pub fn num_bytes(self) -> usize {
        match self {
            Digest::Sha256 => 256 / 8,
            Digest::Sha512 => 512 / 8,
        }
    }
}
//////////////////////////////////////////////////////////////////////////////////

const MAX_FRAME_SIZE: usize = 1024 * 1024 * 8;

/// Config for Secio
#[derive(Clone)]
pub struct Config {
    pub(crate) key: Keypair,
    pub(crate) agreements_proposal: Option<String>,
    pub(crate) ciphers_proposal: Option<String>,
    pub(crate) digests_proposal: Option<String>,
    pub(crate) max_frame_length: usize,
}

impl Config {
    /// Create config
    pub fn new(key_pair: Keypair) -> Self {
        Config {
            key: key_pair,
            agreements_proposal: None,
            ciphers_proposal: None,
            digests_proposal: None,
            max_frame_length: MAX_FRAME_SIZE,
        }
    }

    /// Max frame length
    pub fn max_frame_length(mut self, size: usize) -> Self {
        self.max_frame_length = size;
        self
    }

    /// Override the default set of supported key agreement algorithms.
    pub fn key_agreements<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item = &'a KeyAgreement>,
    {
        self.agreements_proposal = Some(support::key_agreements_proposition(xs));
        self
    }

    /// Override the default set of supported ciphers.
    pub fn ciphers<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item = &'a CipherType>,
    {
        self.ciphers_proposal = Some(support::ciphers_proposition(xs));
        self
    }

    /// Override the default set of supported digest algorithms.
    pub fn digests<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item = &'a Digest>,
    {
        self.digests_proposal = Some(support::digests_proposition(xs));
        self
    }

    /// Attempts to perform a handshake on the given socket.
    ///
    /// On success, produces a `SecureStream` that can then be used to encode/decode
    /// communications, plus the public key of the remote, plus the ephemeral public key.
    pub async fn handshake<T>(self, socket: T) -> Result<(SecureStream<T>, PublicKey, EphemeralPublicKey), SecioError>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        handshake(socket, self).await
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/secio/1.0.0"]
    }
}

async fn make_secure_output<T>(config: Config, socket: T) -> Result<SecioOutput<T>, TransportError>
where
    T: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    // TODO: to be more elegant, local private key could be returned by handshake()
    let pri_key = config.key.clone();
    let la = socket.local_multiaddr();
    let ra = socket.remote_multiaddr();

    let (stream, remote_pub_key, _ephemeral_public_key) = config.handshake(socket).await?;
    let output = SecioOutput {
        stream,
        la,
        ra,
        local_priv_key: pri_key.clone(),
        local_peer_id: pri_key.public().into(),
        remote_pub_key: remote_pub_key.clone(),
        remote_peer_id: remote_pub_key.into(),
    };
    Ok(output)
}

#[async_trait]
impl<T> Upgrader<T> for Config
where
    T: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = SecioOutput<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }
}

/// Output of the secio protocol. It implements the SecureStream trait
#[pin_project::pin_project]
pub struct SecioOutput<S> {
    /// The encrypted stream.
    #[pin]
    pub stream: SecureStream<S>,
    /// The local multiaddr of the connection
    la: Multiaddr,
    /// The remote multiaddr of the connection
    ra: Multiaddr,
    /// The private key of the local
    pub local_priv_key: Keypair,
    /// For convenience, the local peer ID, generated from local pub key
    pub local_peer_id: PeerId,
    /// The public key of the remote.
    pub remote_pub_key: PublicKey,
    /// For convenience, put a PeerId here, which is actually calculated from remote_key
    pub remote_peer_id: PeerId,
}

impl<S: ConnectionInfo> ConnectionInfo for SecioOutput<S> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<S> SecureInfo for SecioOutput<S> {
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

impl<S: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncRead for SecioOutput<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.stream.poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncWrite for SecioOutput<S> {
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

impl From<SecioError> for TransportError {
    fn from(e: SecioError) -> Self {
        // TODO: make a security error catalog for secio
        TransportError::SecurityError(Box::new(e))
    }
}
