use crate::{NoiseConfig, NoiseOutput, Protocol, RemoteIdentity, X25519Spec, X25519, XX};
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
use zeroize::Zeroize;

#[pin_project::pin_project]
pub struct NoiseStream<T> {
    #[pin]
    io: NoiseOutput<T>,
    la: Multiaddr,
    ra: Multiaddr,
    local_priv_key: Keypair,
    remote_pub_key: Option<PublicKey>,
}

impl<T: Send> ConnectionInfo for NoiseStream<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<T> SecureInfo for NoiseStream<T> {
    fn local_peer(&self) -> PeerId {
        self.local_priv_key.public().into_peer_id()
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_pub_key.clone().unwrap().into_peer_id()
    }

    fn local_priv_key(&self) -> Keypair {
        self.local_priv_key.clone()
    }

    fn remote_pub_key(&self) -> PublicKey {
        self.remote_pub_key.clone().unwrap()
    }
}

impl<T: AsyncRead + Send + Unpin + 'static> AsyncRead for NoiseStream<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.project().io.poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Send + Unpin + 'static> AsyncWrite for NoiseStream<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.project().io.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_close(cx)
    }
}

impl UpgradeInfo for NoiseConfig<XX, X25519> {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/noise/xx/25519/chachapoly/sha256/0.1.0"]
    }
}

impl UpgradeInfo for NoiseConfig<XX, X25519Spec> {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/noise"]
    }
}

#[async_trait::async_trait]
impl<T, C> Upgrader<T> for NoiseConfig<XX, C>
where
    Self: UpgradeInfo,
    T: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = NoiseStream<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let la = socket.local_multiaddr();
        let ra = socket.remote_multiaddr();

        let local_priv_key = self.local_priv_key.clone();

        let (id, out) = self.handshake(socket, false).await?;

        let remote_pub_key = {
            if let RemoteIdentity::IdentityKey(pub_key) = id {
                Some(pub_key)
            } else {
                None
            }
        };

        Ok(NoiseStream {
            io: out,
            la,
            ra,
            local_priv_key,
            remote_pub_key,
        })
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let la = socket.local_multiaddr();
        let ra = socket.remote_multiaddr();

        let local_priv_key = self.local_priv_key.clone();

        let (id, out) = self.handshake(socket, true).await?;

        let remote_pub_key = {
            if let RemoteIdentity::IdentityKey(pub_key) = id {
                Some(pub_key)
            } else {
                None
            }
        };

        Ok(NoiseStream {
            io: out,
            la,
            ra,
            local_priv_key,
            remote_pub_key,
        })
    }
}
