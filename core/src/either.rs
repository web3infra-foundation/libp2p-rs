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
/*
impl<A, B> StreamMuxer for EitherOutput<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Substream = EitherOutput<A::Substream, B::Substream>;
    type OutboundSubstream = EitherOutbound<A, B>;
    type Error = IoError;

    fn poll_event(&self, cx: &mut Context) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.poll_event(cx).map(|result| {
                result.map_err(|e| e.into()).map(|event| {
                    match event {
                        StreamMuxerEvent::AddressChange(addr) => StreamMuxerEvent::AddressChange(addr),
                        StreamMuxerEvent::InboundSubstream(substream) =>
                            StreamMuxerEvent::InboundSubstream(EitherOutput::First(substream))
                    }
                })
            }),
            EitherOutput::Second(inner) => inner.poll_event(cx).map(|result| {
                result.map_err(|e| e.into()).map(|event| {
                    match event {
                        StreamMuxerEvent::AddressChange(addr) => StreamMuxerEvent::AddressChange(addr),
                        StreamMuxerEvent::InboundSubstream(substream) =>
                            StreamMuxerEvent::InboundSubstream(EitherOutput::Second(substream))
                    }
                })
            }),
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        match self {
            EitherOutput::First(inner) => EitherOutbound::A(inner.open_outbound()),
            EitherOutput::Second(inner) => EitherOutbound::B(inner.open_outbound()),
        }
    }

    fn poll_outbound(&self, cx: &mut Context, substream: &mut Self::OutboundSubstream) -> Poll<Result<Self::Substream, Self::Error>> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutbound::A(ref mut substream)) => {
                inner.poll_outbound(cx, substream).map(|p| p.map(EitherOutput::First)).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutbound::B(ref mut substream)) => {
                inner.poll_outbound(cx, substream).map(|p| p.map(EitherOutput::Second)).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        match self {
            EitherOutput::First(inner) => {
                match substream {
                    EitherOutbound::A(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(inner) => {
                match substream {
                    EitherOutbound::B(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }

    fn read_substream(&self, cx: &mut Context, sub: &mut Self::Substream, buf: &mut [u8]) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn write_substream(&self, cx: &mut Context, sub: &mut Self::Substream, buf: &[u8]) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn flush_substream(&self, cx: &mut Context, sub: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn shutdown_substream(&self, cx: &mut Context, sub: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        match self {
            EitherOutput::First(inner) => {
                match substream {
                    EitherOutput::First(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(inner) => {
                match substream {
                    EitherOutput::Second(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }

    fn close(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.close(cx).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.close(cx).map_err(|e| e.into()),
        }
    }

    fn flush_all(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.flush_all(cx).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.flush_all(cx).map_err(|e| e.into()),
        }
    }
}
*/

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
