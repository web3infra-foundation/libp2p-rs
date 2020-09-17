//! This crate implements the [Yamux specification][1].
//!
//! It multiplexes independent I/O streams over reliable, ordered connections,
//! such as TCP/IP.
//!
//! The three primary objects, clients of this crate interact with, are:
//!
//! - [`Connection`], which wraps the underlying I/O resource, e.g. a socket,
//! - [`Stream`], which implements [`futures::io::AsyncRead`] and
//!   [`futures::io::AsyncWrite`], and
//! - [`Control`], to asynchronously control the [`Connection`].
//!
//! [1]: https://github.com/hashicorp/yamux/blob/master/spec.md

#![forbid(unsafe_code)]

mod chunks;
mod error;
mod frame;
mod pause;

#[cfg(test)]
mod tests;

pub(crate) mod connection;

use async_trait::async_trait;
use futures::prelude::*;
use log::{info, trace};
use std::fmt;

pub use crate::connection::{Connection, Control, Mode, Stream};
pub use crate::error::ConnectionError;
pub use crate::frame::{
    header::{HeaderDecodeError, StreamId},
    FrameDecodeError,
};
use futures::future::BoxFuture;
use libp2p_core::identity::Keypair;
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::TransportError;
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_core::{PeerId, PublicKey};
use libp2p_traits::{Read2, Write2};

const DEFAULT_CREDIT: u32 = 256 * 1024; // as per yamux specification

/// Specifies when window update frames are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowUpdateMode {
    /// Send window updates as soon as a [`Stream`]'s receive window drops to 0.
    ///
    /// This ensures that the sender can resume sending more data as soon as possible
    /// but a slow reader on the receiving side may be overwhelmed, i.e. it accumulates
    /// data in its buffer which may reach its limit (see `set_max_buffer_size`).
    /// In this mode, window updates merely prevent head of line blocking but do not
    /// effectively exercise back pressure on senders.
    OnReceive,

    /// Send window updates only when data is read on the receiving end.
    ///
    /// This ensures that senders do not overwhelm receivers and keeps buffer usage
    /// low. However, depending on the protocol, there is a risk of deadlock, namely
    /// if both endpoints want to send data larger than the receivers window and they
    /// do not read before finishing their writes. Use this mode only if you are sure
    /// that this will never happen, i.e. if
    ///
    /// - Endpoints *A* and *B* never write at the same time, *or*
    /// - Endpoints *A* and *B* write at most *n* frames concurrently such that the sum
    ///   of the frame lengths is less or equal to the available credit of *A* and *B*
    ///   respectively.
    OnRead,
}

/// Yamux configuration.
///
/// The default configuration values are as follows:
///
/// - receive window = 256 KiB
/// - max. buffer size (per stream) = 1 MiB
/// - max. number of streams = 8192
/// - window update mode = on receive
/// - read after close = true
/// - lazy open = false
#[derive(Debug, Clone)]
pub struct Config {
    receive_window: u32,
    max_buffer_size: usize,
    max_num_streams: usize,
    window_update_mode: WindowUpdateMode,
    read_after_close: bool,
    lazy_open: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            receive_window: DEFAULT_CREDIT,
            max_buffer_size: 1024 * 1024,
            max_num_streams: 8192,
            window_update_mode: WindowUpdateMode::OnReceive,
            read_after_close: true,
            lazy_open: false,
        }
    }
}

impl Config {
    /// make a default yamux config
    ///
    pub fn new() -> Self {
        Config::default()
    }
    /// Set the receive window (must be >= 256 KiB).
    ///
    /// # Panics
    ///
    /// If the given receive window is < 256 KiB.
    pub fn set_receive_window(&mut self, n: u32) -> &mut Self {
        assert!(n >= DEFAULT_CREDIT);
        self.receive_window = n;
        self
    }

    /// Set the max. buffer size per stream.
    pub fn set_max_buffer_size(&mut self, n: usize) -> &mut Self {
        self.max_buffer_size = n;
        self
    }

    /// Set the max. number of streams.
    pub fn set_max_num_streams(&mut self, n: usize) -> &mut Self {
        self.max_num_streams = n;
        self
    }

    /// Set the window update mode to use.
    pub fn set_window_update_mode(&mut self, m: WindowUpdateMode) -> &mut Self {
        self.window_update_mode = m;
        self
    }

    /// Allow or disallow streams to read from buffered data after
    /// the connection has been closed.
    pub fn set_read_after_close(&mut self, b: bool) -> &mut Self {
        self.read_after_close = b;
        self
    }

    /// Enable or disable the sending of an initial window update frame
    /// when opening outbound streams.
    ///
    /// When enabled, opening a new outbound stream will not result in an
    /// immediate send of a frame, instead the first outbound data frame
    /// will be marked as opening a stream.
    ///
    /// When disabled (the current default), opening a new outbound
    /// stream will result in a window update frame being sent immediately
    /// to the remote. This allows opening a stream with a custom receive
    /// window size (cf. [`Config::set_receive_window`]) which the remote
    /// can directly make use of.
    pub fn set_lazy_open(&mut self, b: bool) -> &mut Self {
        self.lazy_open = b;
        self
    }
}

// Check that we can safely cast a `usize` to a `u64`.
static_assertions::const_assert! {
    std::mem::size_of::<usize>() <= std::mem::size_of::<u64>()
}

// Check that we can safely cast a `u32` to a `usize`.
static_assertions::const_assert! {
    std::mem::size_of::<u32>() <= std::mem::size_of::<usize>()
}

/// A Yamux connection.
///
/// This implementation isn't capable of detecting when the underlying socket changes its address,
/// and no [`StreamMuxerEvent::AddressChange`] event is ever emitted.
pub struct Yamux<C> {
    /// The [`futures::stream::Stream`] of incoming substreams.
    connection: Option<Connection<C>>,
    /// Handle to control the connection.
    control: Control,
}

impl<C> fmt::Debug for Yamux<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Yamux")
    }
}

impl<C: Read2 + Write2 + Unpin + Send + 'static> Yamux<C> {
    /// Create a new Yamux connection.
    pub fn new(io: C, mut cfg: Config, mode: Mode) -> Self {
        cfg.set_read_after_close(false);
        let conn = Connection::new(io, cfg, mode);
        let control = conn.control();
        Yamux {
            connection: Some(conn),
            control,
        }
    }
}

impl<C> SecureInfo for Yamux<C> {
    fn local_peer(&self) -> PeerId {
        unimplemented!()
    }

    fn remote_peer(&self) -> PeerId {
        unimplemented!()
    }

    fn local_priv_key(&self) -> Keypair {
        unimplemented!()
    }

    fn remote_pub_key(&self) -> PublicKey {
        unimplemented!()
    }
}

#[async_trait]
impl<C: Read2 + Write2 + Unpin + Send + 'static> StreamMuxer for Yamux<C> {
    type Substream = Stream;

    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("opening a new outbound substream for yamux...");
        let s = self.control.open_stream().await?;
        Ok(s)
    }

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("opening a new outbound substream for yamux...");
        let s = self.control.accept_stream().await?;
        Ok(s)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.control.close().await?;
        Ok(())
    }

    // fn take_inner_stream(&mut self) -> Option<BoxStream<'static, Result<Self::Substream, TransportError>>> {
    //     let stream = self.0.incoming.take();
    //     stream
    // }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        if let Some(mut conn) = self.connection.take() {
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
        vec![b"/yamux/1.0.0"]
    }
}

#[async_trait]
impl<T> Upgrader<T> for Config
where
    T: Read2 + Write2 + Send + Unpin + 'static,
{
    type Output = Yamux<T>;

    async fn upgrade_inbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        trace!("upgrading yamux inbound");
        Ok(Yamux::new(socket, self, Mode::Server))
    }

    async fn upgrade_outbound(
        self,
        socket: T,
        _info: <Self as UpgradeInfo>::Info,
    ) -> Result<Self::Output, TransportError> {
        trace!("upgrading yamux outbound");
        Ok(Yamux::new(socket, self, Mode::Client))
    }
}

// impl Into<TransportError> for ConnectionError {
//     fn into(self: ConnectionError) -> TransportError {
//         TransportError::Internal
//     }
// }

impl From<ConnectionError> for TransportError {
    fn from(_: ConnectionError) -> Self {
        // TODO: make a mux error catalog for secio
        TransportError::Internal
    }
}
