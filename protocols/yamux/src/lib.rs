// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

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
use std::fmt;
use log::{trace};
use futures::prelude::*;
use futures::stream::BoxStream;

pub use crate::connection::{into_stream, Connection, Control, Mode, Stream};
pub use crate::error::ConnectionError;
pub use crate::frame::{
    header::{HeaderDecodeError, StreamId},
    FrameDecodeError,
};
use libp2p_traits::{Write2, Read2};
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_core::transport::TransportError;
use libp2p_core::muxing::StreamMuxer;
use futures::{StreamExt, TryStreamExt, SinkExt};
use futures::future::BoxFuture;
use futures::channel::mpsc;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::identity::Keypair;
use libp2p_core::{PublicKey, PeerId};

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
pub struct Yamux(Inner);

impl fmt::Debug for Yamux {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Yamux")
    }
}

struct Inner {
    /// The [`futures::stream::Stream`] of incoming substreams.
    incoming: Option<BoxStream<'static, Result<Stream, TransportError>>>,
    /// Handle to control the connection.
    control: Control,

    stream_sender: mpsc::Sender<Stream>,
    stream_receiver: mpsc::Receiver<Stream>,
}


impl Yamux
{
    /// Create a new Yamux connection.
    pub fn new<C>(io: C, mut cfg: Config, mode: Mode) -> Self
        where C: Read2 + Write2 + Send + Unpin + 'static
    {
        cfg.set_read_after_close(false);
        let conn = Connection::new(io, cfg, mode);
        let ctrl = conn.control();
        let (tx, rx) = mpsc::channel(1);
        let inner = Inner {
            incoming: Some(into_stream(conn).err_into().boxed()),
            control: ctrl,
            stream_sender: tx,
            stream_receiver: rx,
        };
        Yamux(inner)
    }
}

impl SecureInfo for Yamux {
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
impl StreamMuxer for Yamux
{
    type Substream = Stream;

    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("opening a new outbound substream for yamux...");
        let s = self.0.control.open_stream().await?;
        Ok(s)
    }

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError> {
        trace!("waiting for a new inbound substream for yamux...");
        self.0.stream_receiver.next().await.ok_or(TransportError::Internal)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        let _ = self.0.control.close().await?;
        Ok(())
    }

    // fn take_inner_stream(&mut self) -> Option<BoxStream<'static, Result<Self::Substream, TransportError>>> {
    //     let stream = self.0.incoming.take();
    //     stream
    // }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>>{
        if let Some(mut incoming) = self.0.incoming.take() {
            trace!("starting yamux main loop...");

            //let tx = self.0.stream_sender.clone();
            // Some(incoming.for_each(move|s| {
            //     let mut tx = tx.clone();
            //     async move {
            //         tx.send(s.unwrap()).await;
            //     }
            // }).boxed())

            let mut tx = self.0.stream_sender.clone();
            Some(
                async move {
                    loop {
                        if let Some(Ok(s)) = incoming.next().await {
                            if tx.send(s).await.is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    tx.close_channel();
                }
            .boxed())

        } else {
            None
        }
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(b"/yamux/1.0.0")
    }
}

#[async_trait]
impl<T> Upgrader<T> for Config
    where T: Read2 + Write2 + Send + Unpin + 'static
{
    type Output = Yamux;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("upgrading yamux inbound");
        Ok(Yamux::new(socket, self, Mode::Server))
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
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