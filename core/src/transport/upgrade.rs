
//! Transport upgrader.
//!
// TODO: add example

use async_trait::async_trait;
use crate::{Multiaddr, Transport, transport::{TransportError}};
use futures_timer::Delay;
use std::{time::Duration};
use futures::{FutureExt, AsyncWrite, AsyncRead, TryFutureExt, TryStreamExt, StreamExt, Stream};
use futures::select;
use futures::prelude::*;
use log::{trace};
use crate::transport::TransportListener;
use crate::upgrade::{OutboundUpgrade, InboundUpgrade};
use std::pin::Pin;
use std::task::{Context, Poll};

/// A `TransportUpgrade` is a `Transport` that wraps another `Transport` and adds
/// upgrade capabilities to all inbound and outbound connection attempts.
///
#[derive(Debug, Copy, Clone)]
pub struct TransportUpgrade<InnerTrans, TSecurityMuxer> {
    inner: InnerTrans,
    security_muxer: TSecurityMuxer,
}

impl<InnerTrans, TSecurityMuxer> TransportUpgrade<InnerTrans, TSecurityMuxer> {
    /// Wraps around a `Transport` to add upgrade capabilities.
    pub fn new(inner: InnerTrans, security_muxer: TSecurityMuxer) -> Self {
        TransportUpgrade {
            inner,
            security_muxer,
        }
    }

/*    /// Upgrades the transport to setup the security.
    ///
    pub fn secure(&self) -> Builder<
        AndThen<T, impl FnOnce(C, ConnectedPoint) -> Authenticate<C, U> + Clone>
    > where
        T: Transport<Output = C>,
        I: ConnectionInfo,
        C: AsyncRead + AsyncWrite + Unpin,
        D: AsyncRead + AsyncWrite + Unpin,
        U: InboundUpgrade<Negotiated<C>, Output = (I, D), Error = E>,
        U: OutboundUpgrade<Negotiated<C>, Output = (I, D), Error = E> + Clone,
        E: Error + 'static,
    {
        let version = self.version;
        Builder::new(self.inner.and_then(move |conn, endpoint| {
            Authenticate {
                inner: upgrade::apply(conn, upgrade, endpoint, version)
            }
        }), version)
    }*/
}

#[async_trait]
impl<InnerTrans, TSecurityMuxer> Transport for TransportUpgrade<InnerTrans, TSecurityMuxer>
where
    InnerTrans: Transport + Send,
    InnerTrans::Error: 'static,
    TSecurityMuxer: InboundUpgrade<InnerTrans::Output, Output = InnerTrans::Output> + OutboundUpgrade<InnerTrans::Output, Output = InnerTrans::Output> + Send,
{
    type Output = InnerTrans::Output;
    type Error = InnerTrans::Error;
    type Listener = ListenerUpgrade<InnerTrans::Listener>;
    type ListenerUpgrade = InnerTrans::ListenerUpgrade;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr)?;

        let listener = ListenerUpgrade {
            inner: listener,
        };

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError<Self::Error>> {
        let stream = self.inner.dial(addr).await?;

        let s = self.security_muxer.upgrade_outbound(stream).await.unwrap();

        Ok(s)
        //self.security.upgrade_outbound(stream, self.security.protocol_info()).await
        //Ok(stream)
    }
}

pub struct ListenerUpgrade<InnerListener> {
    inner: InnerListener,
    // TODO: add threshold support here
}

impl<InnerListener> ListenerUpgrade<InnerListener> {
    pub fn incoming(&mut self) -> Incoming<'_, InnerListener> {
        Incoming(self)
    }
}

pub struct Incoming<'a, Inner>(&'a mut ListenerUpgrade<Inner>);

impl<'a, Inner > Stream for Incoming<'a, Inner>
where Inner: TransportListener
{
    type Item = Result<Inner::Output, TransportError<Inner::Error>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let future = self.0.inner.accept();
        futures::pin_mut!(future);

        let socket = futures::ready!(future.poll(cx))?;
        Poll::Ready(Some(Ok(socket)))
    }
}

#[async_trait]
impl<InnerListener: TransportListener + Send + Sync> TransportListener for ListenerUpgrade<InnerListener> {
    type Output = InnerListener::Output;
    type Error = InnerListener::Error;

    async fn accept(&mut self) -> Result<Self::Output, TransportError<Self::Error>> {

        // //self.inner.accept().and_then().
        //
        // let mut stream = futures::stream::unfold(0u32, |s| async {
        //     match self.inner.accept().await {
        //         Ok(st) => {
        //             Some((0, s))
        //         },
        //         Err(_e) => None
        //     }
        // });
        //


        //self.accept().await

        let mut s = self.incoming().then(|r|async {r}.boxed());
        //let mut ss = s.for_each_concurrent(None,|t|async {t}).into_stream();

        let socket = s.next().await.unwrap().unwrap();


        Err(TransportError::Timeout)
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}

