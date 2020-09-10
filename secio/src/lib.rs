//! Aes Encrypted communication and handshake process implementation

#![deny(missing_docs)]

use async_trait::async_trait;

use crate::{
    crypto::cipher::CipherType, error::SecioError, exchange::KeyAgreement,
    handshake::procedure::handshake,
};

use libp2p_core::identity::Keypair;
use libp2p_core::PublicKey;

use crate::codec::secure_stream::SecureStream;
use futures::{AsyncRead, AsyncWrite};
use libp2p_core::upgrade::{Upgrader, UpgradeInfo};
use libp2p_core::transport::TransportError;
use libp2p_traits::{Read2, Write2};


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
    pub async fn handshake<T>(
        self,
        socket: T,
    ) -> Result<(SecureStream<T>, PublicKey, EphemeralPublicKey), SecioError>
        where
            T: Read2 + Write2 + Send + 'static,
    {
        handshake(socket, self).await
    }
}


/// Output of the secio protocol.
pub struct SecioOutput<S>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    /// The encrypted stream.
    pub stream: SecureStream<S>,
    /// The public key of the remote.
    pub remote_key: PublicKey,
    /// Ephemeral public key used during the negotiation.
    pub ephemeral_public_key: Vec<u8>,
}

impl UpgradeInfo for Config
{
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(b"/secio/1.0.0")
    }
}

#[async_trait]
impl<T> Upgrader<T> for Config
    where T: Read2 + Write2 + Send + Unpin + 'static
{
    type Output = SecureStream<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let (handle, _, _) = self.handshake(socket).await?;
        Ok(handle)
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let (handle, _, _) = self.handshake(socket).await?;
        Ok(handle)
    }
}
/*
impl<S> AsyncRead for SecioOutput<S>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8])
                 -> Poll<Result<usize, io::Error>>
    {
        AsyncRead::poll_read(Pin::new(&mut self.stream), cx, buf)
    }
}

impl<S> AsyncWrite for SecioOutput<S>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8])
                  -> Poll<Result<usize, io::Error>>
    {
        AsyncWrite::poll_write(Pin::new(&mut self.stream), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context)
                  -> Poll<Result<(), io::Error>>
    {
        AsyncWrite::poll_flush(Pin::new(&mut self.stream), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context)
                  -> Poll<Result<(), io::Error>>
    {
        AsyncWrite::poll_close(Pin::new(&mut self.stream), cx)
    }
}*/

impl From<SecioError> for TransportError {
    fn from(_: SecioError) -> Self {
        // TODO: make a security error catalog for secio
        TransportError::Internal
    }
}



