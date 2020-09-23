use std::io;
use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};

use crate::ProtocolId;
use crate::connection::ConnectionId;

#[derive(Debug)]
pub struct Substream<TStream> {
    /// The inner sub stream, created by the StreamMuxer
    inner: TStream,
    /// The protocol of this sub stream
    protocol: ProtocolId,
    /// The connection ID of the sub stream
    /// IT can be used to back track to the stream muxer
    connection_id: ConnectionId,
}

impl<TStream> Substream<TStream> {
    pub(crate) fn new(inner: TStream, protocol: ProtocolId, connection_id: ConnectionId) -> Self {
        Self {
            inner,
            protocol,
            connection_id,
        }
    }
    /// Returns the protocol of the sub stream
    pub fn protocol(&self) -> ProtocolId {
        self.protocol
    }
    /// Returns the connection id of the sub stream
    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
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
