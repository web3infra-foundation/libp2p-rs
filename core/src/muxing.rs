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

use async_trait::async_trait;
use futures::future::BoxFuture;
use libp2p_traits::{ReadEx, WriteEx};

use crate::secure_io::SecureInfo;
use crate::transport::{ConnectionInfo, TransportError};
use futures::io::Error;

/// Information about a stream.
pub trait StreamInfo: Send {
    /// Returns the identity of the stream.
    fn id(&self) -> usize;
}

/// The trait for IReadWrite. It can be made into a trait object `IReadWrite` used
/// by Swarm Substream.
/// `StreamInfo` must be supported.
#[async_trait]
pub trait ReadWriteEx: ReadEx + WriteEx + StreamInfo + Unpin + std::fmt::Debug {
    fn box_clone(&self) -> IReadWrite;
}

pub type IReadWrite = Box<dyn ReadWriteEx>;

impl Clone for IReadWrite {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Wrapper for IReadWrite supporting ReadEx + WriteEx
pub struct WrapIReadWrite {
    inner: IReadWrite,
}

impl From<IReadWrite> for WrapIReadWrite {
    fn from(inner: IReadWrite) -> Self {
        Self { inner }
    }
}
impl Into<IReadWrite> for WrapIReadWrite {
    fn into(self) -> IReadWrite {
        self.inner
    }
}

#[async_trait]
impl ReadEx for WrapIReadWrite {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.inner.read2(buf).await
    }
}
#[async_trait]
impl WriteEx for WrapIReadWrite {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.inner.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        self.inner.flush2().await
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.inner.close2().await
    }
}

#[async_trait]
pub trait StreamMuxer {
    /// Opens a new outgoing substream.
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError>;
    /// Accepts a new incoming substream.
    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError>;
    /// Closes the stream muxer, the task of stream muxer will then exit.
    async fn close(&mut self) -> Result<(), TransportError>;
    /// Returns a Future which represents the main loop of the stream muxer.
    fn task(&mut self) -> Option<BoxFuture<'static, ()>>;
    /// Returns the cloned Trait object.
    fn box_clone(&self) -> IStreamMuxer;
}

/// The trait for IStreamMuxer. It can be made into a trait object `IStreamMuxer`.
/// Stream muxer in Swarm must support ConnectionInfo + SecureInfo.
pub trait StreamMuxerEx: StreamMuxer + ConnectionInfo + SecureInfo + std::fmt::Debug {}

pub type IStreamMuxer = Box<dyn StreamMuxerEx>;

impl Clone for IStreamMuxer {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}
