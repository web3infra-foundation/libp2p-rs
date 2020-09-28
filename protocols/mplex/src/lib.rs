pub mod connection;
pub mod error;
mod frame;
mod pause;

use async_trait::async_trait;
use futures::FutureExt;
use log::{info, trace};
use std::fmt;

use libp2p_core::muxing::{StreamInfo, StreamMuxer};
use libp2p_core::transport::{ConnectionInfo, TransportError};
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_traits::{ReadEx, WriteEx};

use crate::connection::Connection;
use connection::{control::Control, stream::Stream, Id};
use error::ConnectionError;
use futures::future::BoxFuture;
use libp2p_core::identity::Keypair;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::{Multiaddr, PeerId, PublicKey};

#[derive(Clone)]
pub struct Config {}

impl Config {
    pub fn new() -> Self {
        Config {}
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// A Mplex connection.
///
/// This implementation isn't capable of detecting when the underlying socket changes its address,
/// and no [`StreamMuxerEvent::AddressChange`] event is ever emitted.
pub struct Mplex<C> {
    /// The [`futures::stream::Stream`] of incoming substreams.
    conn: Option<Connection<C>>,
    /// Handle to control the connection.
    ctrl: Control,
    /// For debug purpose
    id: Id,
    /// The secure&connection info provided by underlying socket.
    /// The socket is moved into Connection, so we have to make a copy of these information
    ///
    /// The local multiaddr of this connection
    pub la: Multiaddr,
    /// The remote multiaddr of this connection
    pub ra: Multiaddr,
    /// The private key of the local
    pub local_priv_key: Keypair,
    /// For convenience, the local peer ID, generated from local pub key
    pub local_peer_id: PeerId,
    /// The public key of the remote.
    pub remote_pub_key: PublicKey,
    /// For convenience, put a PeerId here, which is actually calculated from remote_key
    pub remote_peer_id: PeerId,
}

impl<C> Clone for Mplex<C> {
    fn clone(&self) -> Self {
        Mplex {
            conn: None,
            ctrl: self.ctrl.clone(),
            id: self.id,
            la: self.la.clone(),
            ra: self.ra.clone(),
            local_priv_key: self.local_priv_key.clone(),
            local_peer_id: self.local_peer_id.clone(),
            remote_pub_key: self.remote_pub_key.clone(),
            remote_peer_id: self.remote_peer_id.clone(),
        }
    }
}

impl<C> fmt::Debug for Mplex<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Mplex").field("Id", &self.id).finish()
    }
}

impl<C: ConnectionInfo + SecureInfo + ReadEx + WriteEx + Unpin + Send + 'static> Mplex<C> {
    pub fn new(io: C) -> Self {
        // `io` will be moved into Connection soon, make a copy of the connection & secure info
        let la = io.local_multiaddr();
        let ra = io.remote_multiaddr();
        let local_priv_key = io.local_priv_key();
        let local_peer_id = io.local_peer();
        let remote_pub_key = io.remote_pub_key();
        let remote_peer_id = io.remote_peer();

        let conn = Connection::new(io);
        let id = conn.id();
        let ctrl = conn.control();
        Mplex {
            conn: Some(conn),
            ctrl,
            id,
            la,
            ra,
            local_priv_key,
            local_peer_id,
            remote_pub_key,
            remote_peer_id,
        }
    }
}

impl<C: Send> ConnectionInfo for Mplex<C> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }
    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<C> SecureInfo for Mplex<C> {
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

/// StreamInfo for Mplex::Stream
impl StreamInfo for Stream {
    fn id(&self) -> usize {
        self.id() as usize
    }
}

#[async_trait]
impl<C: ReadEx + WriteEx + Unpin + Send + 'static> StreamMuxer for Mplex<C> {
    type Substream = Stream;

    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("opening a new outbound substream for mplex...");
        let s = self.ctrl.open_stream().await?;
        Ok(s)
    }

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("waiting for a new inbound substream for yamux...");
        self.ctrl
            .accept_stream()
            .await
            .or(Err(TransportError::Internal))
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        let _ = self.ctrl.close().await?;
        Ok(())
    }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        if let Some(mut conn) = self.conn.take() {
            return Some(
                async move {
                    while conn.next_stream().await.is_ok() {}
                    info!("connection is closed");
                }
                .boxed(),
            );
        }
        None
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/mplex/1.0.0"]
    }
}

#[async_trait]
impl<T> Upgrader<T> for Config
where
    T: ConnectionInfo + SecureInfo + ReadEx + WriteEx + Send + Unpin + 'static,
{
    type Output = Mplex<T>;

    async fn upgrade_inbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        trace!("upgrading mplex inbound");
        Ok(Mplex::new(socket))
    }

    async fn upgrade_outbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        trace!("upgrading mplex outbound");
        Ok(Mplex::new(socket))
    }
}

impl From<ConnectionError> for TransportError {
    fn from(_: ConnectionError) -> Self {
        // TODO: make a mux error catalog for secio
        TransportError::StreamMuxerError
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
