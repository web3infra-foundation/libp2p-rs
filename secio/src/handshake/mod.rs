/// Most of the code for this module comes from `rust-libp2p`, but modified some logic(struct).

use async_trait::async_trait;

use crate::{
    crypto::cipher::CipherType, error::SecioError, exchange::KeyAgreement,
    handshake::procedure::handshake, support, Digest, EphemeralPublicKey,
};

use libp2p_core::identity::Keypair;
use libp2p_core::PublicKey;

use crate::codec::secure_stream::SecureStream;
use futures::{AsyncRead, AsyncWrite};
use libp2p_core::upgrade::Upgrader;
use libp2p_core::transport::TransportError;
use std::iter;

mod handshake_context;
mod procedure;

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
        T: AsyncRead + AsyncWrite + Send + 'static + Unpin,
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

#[async_trait]
impl<T> Upgrader<T> for Config
where T: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;
    type Output = SecureStream<T>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/secio/1.0.0")
    }

    async fn upgrade_inbound(self, socket: T) -> Result<Self::Output, TransportError> {
        let (handle, _, _) = self.handshake(socket).await.unwrap();
        Ok(handle)
    }

    async fn upgrade_outbound(self, socket: T) -> Result<Self::Output, TransportError> {
        let (handle, _, _) = self.handshake(socket).await.unwrap();
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
