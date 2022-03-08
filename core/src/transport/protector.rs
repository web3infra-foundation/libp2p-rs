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

//! The `pnet` protocol implements *Pre-shared Key Based Private Networks in libp2p*,
//! as specified in [the spec](https://github.com/libp2p/specs/blob/master/pnet/Private-Networks-PSK-V1.md)
//!
//! Libp2p nodes configured with a pre-shared key can only communicate with other nodes with
//! the same key.

use crate::pnet::{Pnet, PnetConfig, PnetOutput};
use crate::transport::{ConnectionInfo, IListener, ITransport, ListenerEvent};
use crate::{
    transport::{TransportError, TransportListener},
    Multiaddr, Transport,
};
use async_trait::async_trait;
use futures::task::{Context, Poll};
use futures::{AsyncRead, AsyncWrite};
use std::{io, pin::Pin};

#[pin_project::pin_project]
pub struct PnetStream<T> {
    #[pin]
    io: PnetOutput<T>,
    la: Multiaddr,
    ra: Multiaddr,
}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncRead for PnetStream<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.project().io.poll_read(cx, buf)
    }
}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> AsyncWrite for PnetStream<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.project().io.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_close(cx)
    }
}

impl<T: Send> ConnectionInfo for PnetStream<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

/// ProtecotrTransport wraps an inner transport, adds the `pnet` support onto of it.
#[derive(Debug, Clone)]
pub struct ProtectorTransport<InnerTrans> {
    inner: InnerTrans,
    pnet: PnetConfig,
}

#[allow(dead_code)]
impl<InnerTrans> ProtectorTransport<InnerTrans> {
    pub fn new(inner: InnerTrans, pnet: PnetConfig) -> Self {
        Self { inner, pnet }
    }
}

#[async_trait]
impl<InnerTrans> Transport for ProtectorTransport<InnerTrans>
where
    InnerTrans: Transport + Clone + 'static,
    InnerTrans::Output: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = PnetStream<InnerTrans::Output>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ProtectorListener::new(inner_listener, self.pnet);
        Ok(Box::new(listener))
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket = self.inner.dial(addr).await?;
        let la = socket.local_multiaddr();
        let ra = socket.remote_multiaddr();
        self.pnet
            .handshake(socket)
            .await
            .map(|io| PnetStream { io, la, ra })
            .map_err(|e| e.into())
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        self.inner.protocols()
    }
}

pub struct ProtectorListener<TOutput> {
    inner: IListener<TOutput>,
    pnet: PnetConfig,
}

impl<TOutput> ProtectorListener<TOutput> {
    pub(crate) fn new(inner: IListener<TOutput>, pnet: PnetConfig) -> Self {
        Self { inner, pnet }
    }
}

#[async_trait]
impl<TOutput> TransportListener for ProtectorListener<TOutput>
where
    TOutput: ConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = PnetStream<TOutput>;

    async fn accept(&mut self) -> Result<ListenerEvent<Self::Output>, TransportError> {
        let e = self.inner.accept().await?;
        match e {
            ListenerEvent::Accepted(o) => {
                let la = o.local_multiaddr();
                let ra = o.remote_multiaddr();
                self.pnet
                    .handshake(o)
                    .await
                    .map(|io| ListenerEvent::Accepted(PnetStream { io, la, ra }))
                    .map_err(|e| e.into())
            }
            ListenerEvent::AddressAdded(a) => Ok(ListenerEvent::AddressAdded(a)),
            ListenerEvent::AddressDeleted(a) => Ok(ListenerEvent::AddressDeleted(a)),
        }
    }

    fn multi_addr(&self) -> Option<&Multiaddr> {
        self.inner.multi_addr()
    }
}
