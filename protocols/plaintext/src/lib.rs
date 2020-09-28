mod codec;
mod error;
mod handshake;
mod secure_stream;

use crate::error::PlaintextError;
use crate::handshake::handshake_plaintext::Remote;
use crate::secure_stream::SecureStream;
use libp2p_core::identity::Keypair;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::TransportError;
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_core::{PeerId, PublicKey};
use libp2p_traits::{ReadEx, WriteEx};
use std::io;

use async_trait::async_trait;

pub mod structs_proto {
    include!(concat!(env!("OUT_DIR"), "/structs_proto.rs"));
}

const MAX_FRAME_SIZE: usize = 1024 * 1024 * 8;

#[derive(Clone)]
pub struct PlainTextConfig {
    pub(crate) key: Keypair,
    pub(crate) max_frame_length: usize,
}

impl PlainTextConfig {
    pub fn new(key: Keypair) -> Self {
        PlainTextConfig {
            key,
            max_frame_length: MAX_FRAME_SIZE,
        }
    }

    pub async fn handshake<T>(self, socket: T) -> Result<(SecureStream<T>, Remote), PlaintextError>
    where
        T: ReadEx + WriteEx + Send + 'static,
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
    T: ReadEx + WriteEx + Send + Unpin + 'static,
{
    type Output = PlainTextOutput<T>;

    async fn upgrade_inbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }

    async fn upgrade_outbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket).await
    }
}

async fn make_secure_output<T>(
    config: PlainTextConfig,
    socket: T,
) -> Result<PlainTextOutput<T>, TransportError>
where
    T: ReadEx + WriteEx + Send + Unpin + 'static,
{
    let pri_key = config.key.clone();

    let (secure_stream, remote) = config.handshake(socket).await?;
    let output = PlainTextOutput {
        stream: secure_stream,
        local_priv_key: pri_key.clone(),
        local_peer_id: pri_key.public().into_peer_id(),
        remote_pub_key: remote.public_key,
        remote_peer_id: remote.peer_id,
    };
    Ok(output)
}

pub struct PlainTextOutput<T> {
    pub stream: SecureStream<T>,
    pub local_priv_key: Keypair,
    /// For convenience, the local peer ID, generated from local pub key
    pub local_peer_id: PeerId,
    /// The public key of the remote.
    pub remote_pub_key: PublicKey,
    /// For convenience, put a PeerId here, which is actually calculated from remote_key
    pub remote_peer_id: PeerId,
}

impl<T> SecureInfo for PlainTextOutput<T> {
    fn local_peer(&self) -> PeerId {
        self.local_peer_id.clone()
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_peer_id.clone()
    }

    fn local_priv_key(&self) -> Keypair {
        self.local_priv_key.clone()
    }

    fn remote_pub_key(&self) -> PublicKey {
        self.remote_pub_key.clone()
    }
}

#[async_trait]
impl<S: ReadEx + WriteEx + Unpin + Send + 'static> ReadEx for PlainTextOutput<S> {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.stream.read2(buf).await
    }
}

#[async_trait]
impl<S: ReadEx + WriteEx + Unpin + Send + 'static> WriteEx for PlainTextOutput<S> {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.stream.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        self.stream.flush2().await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        self.stream.close2().await
    }
}

impl From<PlaintextError> for TransportError {
    fn from(_: PlaintextError) -> Self {
        TransportError::SecurityError
    }
}
