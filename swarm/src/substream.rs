use std::{io, fmt};
use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};

use crate::ProtocolId;
use crate::connection::{ConnectionId, Direction};
use libp2p_core::muxing::StreamInfo;
use libp2p_core::upgrade::ProtocolName;


/// The Id of sub stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

#[derive(Clone)]
pub struct Substream<TStream> {
    /// The inner sub stream, created by the StreamMuxer
    inner: TStream,
    /// The protocol of this sub stream
    protocol: ProtocolId,
    /// The direction of the sub stream
    dir: Direction,
    /// The connection ID of the sub stream
    /// IT can be used to back track to the stream muxer
    cid: ConnectionId,
}

impl<TStream: fmt::Debug> fmt::Debug for Substream<TStream> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Substream")
            .field("inner", &self.inner)
            .field("protocol", &self.protocol.protocol_name_str())
            .field("dir", &self.dir)
            .field("cid", &self.cid)
            .finish()
    }
}

impl<TStream: StreamInfo> Substream<TStream> {
    pub(crate) fn new(inner: TStream, dir: Direction, protocol: ProtocolId, cid: ConnectionId) -> Self {
        Self {
            inner,
            protocol,
            dir,
            cid,
        }
    }
    /// Returns the protocol of the sub stream
    pub fn protocol(&self) -> ProtocolId {
        self.protocol
    }
    /// Returns the connection id of the sub stream
    pub fn cid(&self) -> ConnectionId {
        self.cid
    }
    /// Returns the sub stream Id
    pub fn id(&self) -> StreamId {
        StreamId(self.inner.id())
    }
    /// Returns the inner sub stream
    pub fn stream(self) -> TStream {
        self.inner
    }
}

#[async_trait]
impl<TStream: Read2 + Send> Read2 for Substream<TStream> {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read2(buf).await
    }
}

#[async_trait]
impl<TStream: Write2 + Send> Write2 for Substream<TStream> {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        self.inner.flush2().await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        self.inner.close2().await
    }
}
