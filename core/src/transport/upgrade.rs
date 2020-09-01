
//! Transport upgrader.
//!
// TODO: add example

use async_trait::async_trait;
use crate::{Multiaddr, Transport, transport::{TransportError}};
use futures_timer::Delay;
use std::{time::Duration};
use futures::{AsyncWrite, AsyncRead, TryFutureExt, TryStreamExt, StreamExt, Stream, SinkExt};
use futures::prelude::*;
use log::{trace};
use crate::transport::TransportListener;
use crate::upgrade::{OutboundUpgrade, InboundUpgrade};
use std::pin::Pin;
use std::task::{Context, Poll};
use libp2p_traits::Write2;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::select;


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
    TSecurityMuxer: Send,
//TSecurityMuxer: InboundUpgrade<InnerTrans::Output, Output = InnerTrans::Output> + OutboundUpgrade<InnerTrans::Output, Output = InnerTrans::Output> + Send,
{
    type Output = InnerTrans::Output;
    type Listener = ListenerUpgrade<InnerTrans::Listener>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        let listener = self.inner.listen_on(addr)?;

        let listener = ListenerUpgrade {
            inner: listener,
        };

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let mut stream = self.inner.dial(addr).await?;

        //let mut stream = self.security_muxer.upgrade_outbound(stream).await.unwrap();

        //stream.write2(b"hello").await?;

        Ok(stream)
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
    type Item = Result<Inner::Output, TransportError>;

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

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {

        // let mut stream = futures::stream::unfold(0u32, |s| async {
        //     match self.inner.accept().await {
        //         Ok(st) => {
        //             Some((0, s))
        //         },
        //         Err(_e) => None
        //     }
        // });

        let (mut tx, mut rx) = mpsc::channel(1);

        //let mut accept = self.inner.accept().fuse();


        // let stream = self.inner.accept().await?;
        // tx.send(stream).await.map_err(|e| TransportError::Unreachable)?;

        loop {
            let _ = select! {
                stream = self.inner.accept().fuse() => {
                    trace!("connected first");
                    let mut stream = stream?;
                    //stream.write2(b"upgrading").await?;
                    trace!("send an upgrade");
                    tx.send(stream).await;
                },
                up = rx.next() => {
                    trace!("got an upgrade");

                    let mut up = up.ok_or(TransportError::Internal)?;


                    //futures_timer::Delay::new(Duration::from_secs(5)).await;

                    trace!("upgrade ready to go");

                    //let mut s = self.security_muxer.upgrade_outbound(up).await.unwrap();
                    //up.write2(b"ready").await?;

                    return Ok(up);
                }
            };



            // if let Some(up) = r {
            //     let mut s = self.accept().await.unwrap();
            //     return Ok(s);
            // }
        }


        //self.accept().await

        // let mut s = self.incoming().then(|r|async {r}.boxed());
        // //let mut ss = s.for_each_concurrent(None,|t|async {t}).into_stream();
        //
        // let socket = s.next().await.unwrap().unwrap();


        Err(TransportError::Timeout)
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::memory::MemoryTransport;

    #[test]
    fn listen() {
        let transport = TransportUpgrade::new(MemoryTransport::default(), 0);
        let mut listener = transport.listen_on("/memory/1639174018481".parse().unwrap()).unwrap();

        futures::executor::block_on(async move {
            let s = listener.accept().await.unwrap();
        });
    }

    #[test]
    fn communicating_between_dialer_and_listener() {

        let msg = [1, 2, 3];

        // Setup listener.
        let rand_port = rand::random::<u64>().saturating_add(1);
        let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();
        let cloned_t1_addr = t1_addr.clone();

        let t1 = TransportUpgrade::new(MemoryTransport::default(), 0);

        let listener = async move {
            let mut listener = t1.listen_on(t1_addr.clone()).unwrap();

            // kingwel, TBD  upgrade memory transport

            // let upgrade = listener.filter_map(|ev| futures::future::ready(
            //     ListenerEvent::into_upgrade(ev.unwrap())
            // )).next().await.unwrap();

            // let mut socket = upgrade.0.await.unwrap();

            let mut socket = listener.accept().await.unwrap();

            let mut buf = [0; 3];
            socket.read_exact(&mut buf).await.unwrap();

            assert_eq!(buf, msg);
        };

        // Setup dialer.

        let t2 = TransportUpgrade::new(MemoryTransport::default(), 0);

        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            socket.write_all(&msg).await.unwrap();
        };

        // Wait for both to finish.

        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}

