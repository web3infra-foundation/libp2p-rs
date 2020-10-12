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

use crate::identity::Keypair;
use crate::muxing::{StreamInfo, StreamMuxer};
use crate::secure_io::SecureInfo;
use crate::transport::{ConnectionInfo, TransportError};
use crate::upgrade::ProtocolName;
use crate::{Multiaddr, PeerId, PublicKey};
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
};
use libp2p_traits::{ReadEx, WriteEx};
use pin_project::pin_project;
use std::{io, io::Error as IoError, pin::Pin, task::Context, task::Poll};

#[pin_project(project = EitherOutputProj)]
#[derive(Debug, Copy, Clone)]
pub enum AsyncEitherOutput<A, B> {
    A(#[pin] A),
    B(#[pin] B),
}

impl<A, B> AsyncRead for AsyncEitherOutput<A, B>
where
    A: AsyncRead,
    B: AsyncRead,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncRead::poll_read(a, cx, buf),
            EitherOutputProj::B(b) => AsyncRead::poll_read(b, cx, buf),
        }
    }

    fn poll_read_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &mut [IoSliceMut<'_>]) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncRead::poll_read_vectored(a, cx, bufs),
            EitherOutputProj::B(b) => AsyncRead::poll_read_vectored(b, cx, bufs),
        }
    }
}

impl<A, B> AsyncWrite for AsyncEitherOutput<A, B>
where
    A: AsyncWrite,
    B: AsyncWrite,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncWrite::poll_write(a, cx, buf),
            EitherOutputProj::B(b) => AsyncWrite::poll_write(b, cx, buf),
        }
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[IoSlice<'_>]) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncWrite::poll_write_vectored(a, cx, bufs),
            EitherOutputProj::B(b) => AsyncWrite::poll_write_vectored(b, cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncWrite::poll_flush(a, cx),
            EitherOutputProj::B(b) => AsyncWrite::poll_flush(b, cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        match self.project() {
            EitherOutputProj::A(a) => AsyncWrite::poll_close(a, cx),
            EitherOutputProj::B(b) => AsyncWrite::poll_close(b, cx),
        }
    }
}
#[derive(Debug, Copy, Clone)]
pub enum EitherOutput<A, B> {
    A(A),
    B(B),
}

#[async_trait]
impl<A, B> ReadEx for EitherOutput<A, B>
where
    A: ReadEx + Send,
    B: ReadEx + Send,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            EitherOutput::A(a) => ReadEx::read2(a, buf).await,
            EitherOutput::B(b) => ReadEx::read2(b, buf).await,
        }
    }
}

#[async_trait]
impl<A, B> WriteEx for EitherOutput<A, B>
where
    A: WriteEx + Send,
    B: WriteEx + Send,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            EitherOutput::A(a) => WriteEx::write2(a, buf).await,
            EitherOutput::B(b) => WriteEx::write2(b, buf).await,
        }
    }

    async fn flush2(&mut self) -> io::Result<()> {
        match self {
            EitherOutput::A(a) => WriteEx::flush2(a).await,
            EitherOutput::B(b) => WriteEx::flush2(b).await,
        }
    }

    async fn close2(&mut self) -> io::Result<()> {
        match self {
            EitherOutput::A(a) => WriteEx::close2(a).await,
            EitherOutput::B(b) => WriteEx::close2(b).await,
        }
    }
}

impl<A, B> SecureInfo for EitherOutput<A, B>
where
    A: SecureInfo,
    B: SecureInfo,
{
    fn local_peer(&self) -> PeerId {
        match self {
            EitherOutput::A(a) => a.local_peer(),
            EitherOutput::B(b) => b.local_peer(),
        }
    }

    fn remote_peer(&self) -> PeerId {
        match self {
            EitherOutput::A(a) => a.remote_peer(),
            EitherOutput::B(b) => b.remote_peer(),
        }
    }

    fn local_priv_key(&self) -> Keypair {
        match self {
            EitherOutput::A(a) => a.local_priv_key(),
            EitherOutput::B(b) => b.local_priv_key(),
        }
    }

    fn remote_pub_key(&self) -> PublicKey {
        match self {
            EitherOutput::A(a) => a.remote_pub_key(),
            EitherOutput::B(b) => b.remote_pub_key(),
        }
    }
}

impl<A, B> StreamInfo for EitherOutput<A, B>
where
    A: StreamInfo,
    B: StreamInfo,
{
    fn id(&self) -> usize {
        match self {
            EitherOutput::A(a) => a.id(),
            EitherOutput::B(b) => b.id(),
        }
    }
}

#[async_trait]
impl<A, B> StreamMuxer for EitherOutput<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
    A::Substream: StreamInfo,
    B::Substream: StreamInfo,
{
    type Substream = EitherOutput<A::Substream, B::Substream>;

    async fn open_stream(&mut self) -> Result<Self::Substream, TransportError> {
        match self {
            EitherOutput::A(a) => Ok(EitherOutput::A(a.open_stream().await?)),
            EitherOutput::B(b) => Ok(EitherOutput::B(b.open_stream().await?)),
        }
    }

    async fn accept_stream(&mut self) -> Result<Self::Substream, TransportError> {
        match self {
            EitherOutput::A(a) => Ok(EitherOutput::A(a.accept_stream().await?)),
            EitherOutput::B(b) => Ok(EitherOutput::B(b.accept_stream().await?)),
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        match self {
            EitherOutput::A(a) => a.close().await,
            EitherOutput::B(b) => b.close().await,
        }
    }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        match self {
            EitherOutput::A(a) => a.task(),
            EitherOutput::B(b) => b.task(),
        }
    }
}

impl<A, B> ConnectionInfo for EitherOutput<A, B>
where
    A: ConnectionInfo,
    B: ConnectionInfo,
{
    fn local_multiaddr(&self) -> Multiaddr {
        match self {
            EitherOutput::A(a) => a.local_multiaddr(),
            EitherOutput::B(b) => b.local_multiaddr(),
        }
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        match self {
            EitherOutput::A(a) => a.remote_multiaddr(),
            EitherOutput::B(b) => b.remote_multiaddr(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EitherName<A, B> {
    A(A),
    B(B),
}

impl<A: ProtocolName, B: ProtocolName> ProtocolName for EitherName<A, B> {
    fn protocol_name(&self) -> &[u8] {
        match self {
            EitherName::A(a) => a.protocol_name(),
            EitherName::B(b) => b.protocol_name(),
        }
    }
}

// #[derive(Debug, Copy, Clone)]
// pub enum EitherTransport<A, B> {
//     A(A),
//     B(B),
// }

// #[async_trait]
// impl<A, B> Transport for EitherTransport<A, B>
// where
//     B: Transport,
//     A: Transport,
// {
//     type Output = EitherOutput<A::Output, B::Output>;
//     type Listener = EitherTransportListener<A::Listener, B::Listener>;

//     fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
//         match self {
//             EitherTransport::A(a) => Ok(EitherTransportListener::A(a.listen_on(addr)?)),
//             EitherTransport::B(b) => Ok(EitherTransportListener::B(b.listen_on(addr)?)),
//         }
//     }

//     async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
//         match self {
//             EitherTransport::A(a) => Ok(EitherOutput::A(a.dial(addr).await?)),
//             EitherTransport::B(b) => Ok(EitherOutput::B(b.dial(addr).await?)),
//         }
//     }
// }

// #[derive(Debug, Copy, Clone)]
// pub enum EitherTransportListener<A, B> {
//     A(A),
//     B(B),
// }
// #[async_trait]
// impl<A, B> TransportListener for EitherTransportListener<A, B>
// where
//     B: TransportListener,
//     A: TransportListener,
// {
//     type Output = EitherOutput<A::Output, B::Output>;

//     async fn accept(&mut self) -> Result<Self::Output, TransportError> {
//         match self {
//             EitherTransportListener::A(a) => Ok(EitherOutput::A(a.accept().await?)),
//             EitherTransportListener::B(b) => Ok(EitherOutput::B(b.accept().await?)),
//         }
//     }

//     fn multi_addr(&self) -> Multiaddr {
//         match self {
//             EitherTransportListener::A(a) => a.multi_addr(),
//             EitherTransportListener::B(b) => b.multi_addr(),
//         }
//     }
// }
