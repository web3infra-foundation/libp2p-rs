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

use crate::transport::TransportError;
use async_trait::async_trait;
use futures::future::BoxFuture;

#[async_trait]
pub trait StreamMuxer: Send + Clone + std::fmt::Debug {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream: Send + std::fmt::Debug;

    /// Opens a new outgoing substream, and produces the equivalent to a future that will be
    /// resolved when it becomes available.
    ///
    /// The API of `OutboundSubstream` is totally opaque, and the object can only be interfaced
    /// through the methods on the `StreamMuxer` trait.
    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError>;

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError>;

    async fn close(&mut self) -> Result<(), TransportError>;

    fn task(&mut self) -> Option<BoxFuture<'static, ()>>;
}
