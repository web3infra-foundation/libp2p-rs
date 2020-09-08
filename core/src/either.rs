// Copyright 2017 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};
use std::io;
use crate::upgrade::ProtocolName;
use crate::muxing::StreamMuxer;
use crate::transport::TransportError;


#[derive(Debug, Copy, Clone)]
pub enum EitherOutput<A, B> {
    A(A),
    B(B),
}

#[async_trait]
impl<A, B> Read2 for EitherOutput<A, B>
where
    A: Read2 + Send,
    B: Read2 + Send,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            EitherOutput::A(a) => Read2::read2(a, buf).await,
            EitherOutput::B(b) => Read2::read2(b, buf).await,
        }
    }
}

#[async_trait]
impl<A, B> Write2 for EitherOutput<A, B>
where
    A: Write2 + Send,
    B: Write2 + Send,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            EitherOutput::A(a) => Write2::write2(a, buf).await,
            EitherOutput::B(b) => Write2::write2(b, buf).await,
        }
    }

    async fn flush2(&mut self) -> io::Result<()> {
        match self {
            EitherOutput::A(a) => Write2::flush2(a).await,
            EitherOutput::B(b) => Write2::flush2(b).await,
        }
    }

    async fn close2(&mut self) -> io::Result<()> {
        match self {
            EitherOutput::A(a) => Write2::close2(a).await,
            EitherOutput::B(b) => Write2::close2(b).await,
        }
    }
}

#[async_trait]
impl<A, B> StreamMuxer for EitherOutput<A, B>
where
    A: StreamMuxer + Send + Sync,
    B: StreamMuxer + Send + Sync,
{
    type Substream = EitherOutput<A::Substream, B::Substream>;

    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError> {
        match self {
            EitherOutput::A(a) => Ok(EitherOutput::A(a.open_stream().await?)),
            EitherOutput::B(b) => Ok(EitherOutput::B(b.open_stream().await?)),
        }
    }

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError> {
        unimplemented!()
    }

    fn start(&self) {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
pub enum EitherName<A, B> { A(A), B(B) }

impl<A: ProtocolName, B: ProtocolName> ProtocolName for EitherName<A, B> {
    fn protocol_name(&self) -> &[u8] {
        match self {
            EitherName::A(a) => a.protocol_name(),
            EitherName::B(b) => b.protocol_name()
        }
    }
}
