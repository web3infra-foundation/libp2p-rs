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

pub mod connection;
pub mod error;
mod frame;
mod pause;

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, FutureExt};
use log::{info, trace};
use std::fmt;

use libp2prs_core::muxing::{IReadWrite, IStreamMuxer, ReadWriteEx, StreamInfo, StreamMuxer, StreamMuxerEx};
use libp2prs_core::transport::{ConnectionInfo, TransportError};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};

use crate::connection::Connection;
use connection::{control::Control, stream::Stream, Id};
use error::ConnectionError;
use futures::future::BoxFuture;
use libp2prs_core::identity::Keypair;
use libp2prs_core::secure_io::SecureInfo;
use libp2prs_core::{Multiaddr, PeerId, PublicKey};
use parking_lot::Mutex;
use std::sync::Arc;

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
    conn: Arc<Mutex<Option<Connection<C>>>>,
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
            conn: self.conn.clone(),
            ctrl: self.ctrl.clone(),
            id: self.id,
            la: self.la.clone(),
            ra: self.ra.clone(),
            local_priv_key: self.local_priv_key.clone(),
            local_peer_id: self.local_peer_id,
            remote_pub_key: self.remote_pub_key.clone(),
            remote_peer_id: self.remote_peer_id,
        }
    }
}

impl<C> fmt::Debug for Mplex<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Mplex")
            .field("Id", &self.id)
            .field("Ra", &self.ra)
            .field("Rid", &self.remote_peer_id)
            .finish()
    }
}

impl<C: ConnectionInfo + SecureInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static> Mplex<C> {
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
            conn: Arc::new(Mutex::new(Some(conn))),
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

/// StreamInfo for Mplex::Stream
impl StreamInfo for Stream {
    fn id(&self) -> usize {
        self.val() as usize
    }
}

#[async_trait]
impl ReadWriteEx for Stream {
    fn box_clone(&self) -> IReadWrite {
        Box::new(self.clone())
    }
}

impl<C: AsyncRead + AsyncWrite + Send + Unpin + 'static> StreamMuxerEx for Mplex<C> {}

#[async_trait]
impl<C: AsyncRead + AsyncWrite + Send + Unpin + 'static> StreamMuxer for Mplex<C> {
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError> {
        trace!("opening a new outbound substream for mplex...");
        let s = self.ctrl.open_stream().await?;
        Ok(Box::new(s))
    }

    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError> {
        trace!("waiting for a new inbound substream for yamux...");
        let s = self.ctrl.accept_stream().await?;
        Ok(Box::new(s))
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        let _ = self.ctrl.close().await?;
        Ok(())
    }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        if let Some(mut conn) = self.conn.lock().take() {
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

    fn box_clone(&self) -> IStreamMuxer {
        Box::new(self.clone())
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/mplex/6.7.0"]
    }
}

#[async_trait]
impl<T> Upgrader<T> for Config
where
    T: ConnectionInfo + SecureInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Mplex<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("upgrading mplex inbound");
        Ok(Mplex::new(socket))
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("upgrading mplex outbound");
        Ok(Mplex::new(socket))
    }
}

impl From<ConnectionError> for TransportError {
    fn from(e: ConnectionError) -> Self {
        // TODO: make a mux error catalog for secio
        TransportError::StreamMuxerError(Box::new(e))
    }
}

#[cfg(test)]
mod tests {
    // #[test]
    // fn it_works() {
    //     assert_eq!(2 + 2, 4);
    // }
}
