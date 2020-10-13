//! Muxing is the process of splitting a connection into multiple substreams.
//!
//! The main item of this module is the `StreamMuxer` trait. An implementation of `StreamMuxer`
//! has ownership of a connection, lets you open and close substreams, and read/write data
//! on open substreams.
//!
//! > **Note**: You normally don't need to use the methods of the `StreamMuxer` directly, as this
//! >           is managed by the library's internals.
//!
//! Each substream of a connection is an isolated stream of data. All the substreams are muxed
//! together so that the data read from or written to each substream doesn't influence the other
//! substreams.
//!
//! In the context of libp2p, each substream can use a different protocol. Contrary to opening a
//! connection, opening a substream is almost free in terms of resources. This means that you
//! shouldn't hesitate to rapidly open and close substreams, and to design protocols that don't
//! require maintaining long-lived channels of communication.
//!
//! > **Example**: The Kademlia protocol opens a new substream for each request it wants to
//! >              perform. Multiple requests can be performed simultaneously by opening multiple
//! >              substreams, without having to worry about associating responses with the
//! >              right request.
//!
//! # Implementing a muxing protocol
//!
//! In order to implement a muxing protocol, create an object that implements the `UpgradeInfo`,
//! `InboundUpgrade` and `OutboundUpgrade` traits. See the `upgrade` module for more information.
//! The `Output` associated type of the `InboundUpgrade` and `OutboundUpgrade` traits should be
//! identical, and should be an object that implements the `StreamMuxer` trait.
//!
//! The upgrade process will take ownership of the connection, which makes it possible for the
//! implementation of `StreamMuxer` to control everything that happens on the wire.

use crate::secure_io::SecureInfo;
use crate::transport::{ConnectionInfo, TransportError};
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::io::Error;
use futures::prelude::*;
use libp2p_traits::{ReadEx, WriteEx};
/// Information about a stream.
pub trait StreamInfo: Send {
    /// Returns the identity of the stream.
    fn id(&self) -> usize;
}
#[async_trait]
pub trait ReadWrite: ReadEx + WriteEx + StreamInfo + Unpin + std::fmt::Debug {
    fn box_clone(&self) -> IReadWrite;
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Error>;
    async fn flush(&mut self) -> Result<(), Error>;
    async fn close(&mut self) -> Result<(), Error>;
}

pub type IReadWrite = Box<dyn ReadWrite>;

impl Clone for IReadWrite {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[async_trait]
impl ReadEx for IReadWrite {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.read(buf).await
    }
}

#[async_trait]
impl WriteEx for IReadWrite {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.write(buf).await
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        self.flush().await
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.close().await
    }
}

#[async_trait]
pub trait StreamMuxer {
    /// Opens a new outgoing substream, and produces the equivalent to a future that will be
    /// resolved when it becomes available.
    ///
    /// The API of `OutboundSubstream` is totally opaque, and the object can only be interfaced
    /// through the methods on the `StreamMuxer` trait.
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError>;

    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError>;

    async fn close(&mut self) -> Result<(), TransportError>;

    fn task(&mut self) -> Option<BoxFuture<'static, ()>>;

    fn box_clone(&self) -> IStreamMuxer;
}

pub trait StreamMuxerEx: StreamMuxer + ConnectionInfo + SecureInfo + std::fmt::Debug {}

pub type IStreamMuxer = Box<dyn StreamMuxerEx>;

impl Clone for IStreamMuxer {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}
