pub mod connection;
pub mod error;
mod frame;
mod pause;

use async_trait::async_trait;
use futures::{channel::mpsc, stream::BoxStream, FutureExt};
use log::{info, trace};
use std::fmt;

use libp2p_core::muxing::StreamMuxer;
use libp2p_core::transport::TransportError;
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_traits::{Read2, Write2};

use crate::connection::Connection;
use connection::{control::Control, stream::Stream};
use error::ConnectionError;
use futures::future::BoxFuture;

#[derive(Clone)]
pub struct Config {}

impl Config {
    pub fn new() -> Self {
        Config {}
    }
}

/// A Yamux connection.
///
/// This implementation isn't capable of detecting when the underlying socket changes its address,
/// and no [`StreamMuxerEvent::AddressChange`] event is ever emitted.
pub struct Mplex<C> {
    /// The [`futures::stream::Stream`] of incoming substreams.
    conn: Option<Connection<C>>,
    /// Handle to control the connection.
    ctrl: Control,
}

impl<C> fmt::Debug for Mplex<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Mplex")
    }
}

impl<C: Read2 + Write2 + Unpin + Send + 'static> Mplex<C> {
    pub fn new(socket: C) -> Self {
        let conn = Connection::new(socket);
        let ctrl = conn.control();
        Mplex {
            conn: Some(conn),
            ctrl,
        }
    }
}

#[async_trait]
impl<C: Read2 + Write2 + Unpin + Send + 'static> StreamMuxer for Mplex<C> {
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

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        if let Some(mut conn) = self.conn.take() {
            return Some(
                async move {
                    while let Ok(_) = conn.next_stream().await {}
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
    T: Read2 + Write2 + Send + Unpin + 'static,
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
        TransportError::Internal
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
