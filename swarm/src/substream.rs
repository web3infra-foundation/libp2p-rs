use std::io;
use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};

use crate::ProtocolId;

#[derive(Debug)]
pub struct Substream<TStream> {
    inner: TStream,
    protocol: ProtocolId,
}

impl<TStream> Substream<TStream> {
    fn new(inner: TStream, protocol: ProtocolId) -> Self {
        Self {
            inner,
            protocol,
        }
    }

    fn protocol(&self) -> ProtocolId {
        self.protocol
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
