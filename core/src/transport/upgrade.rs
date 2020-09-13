
//! Transport upgrader.
//!
// TODO: add example

use async_trait::async_trait;
use futures_timer::Delay;
use futures::{StreamExt, Stream, SinkExt, TryStreamExt, FutureExt};
use futures::prelude::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{time::Duration};
use libp2p_traits::{Write2, Read2};
use futures::select;
use pin_project::{pin_project, project};
use log::{trace};
use crate::{Multiaddr, Transport, transport::{TransportError}};
use crate::transport::TransportListener;
use crate::upgrade::Upgrader;
use futures::stream::FuturesUnordered;
use std::num::NonZeroUsize;
use crate::upgrade::multistream::Multistream;
use crate::muxing::StreamMuxer;
use crate::secure_io::SecureInfo;


/// A `TransportUpgrade` is a `Transport` that wraps another `Transport` and adds
/// upgrade capabilities to all inbound and outbound connection attempts.
///
#[derive(Debug, Clone)]
pub struct TransportUpgrade<InnerTrans, TMux, TSec> {
    inner: InnerTrans,

    // protector: Option<TProtector>,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
}

impl<InnerTrans, TMux, TSec> TransportUpgrade<InnerTrans, TMux, TSec>
where
    InnerTrans: Transport,
    InnerTrans::Output: Read2 + Write2 + Unpin,
    TSec: Upgrader<InnerTrans::Output>,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer
{
    /// Wraps around a `Transport` to add upgrade capabilities.
    pub fn new(inner: InnerTrans, mux: TMux, sec: TSec) -> Self {
        TransportUpgrade {
            inner,
            sec: Multistream::new(sec),
            mux: Multistream::new(mux),
        }
    }
}

#[pin_project]
pub struct ListenerUpgrade<InnerListener, TMux, TSec>
where
    InnerListener: TransportListener + Unpin,
    InnerListener::Output: Read2 + Write2 + Unpin,
    TSec: Upgrader<InnerListener::Output>,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
{
    #[pin]
    inner: InnerListener,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
    futures: FuturesUnordered<Pin<Box<Future<Output = Result<TMux::Output, TransportError>> + Send>>>,
    limit: Option<NonZeroUsize>,
    // TODO: add threshold support here
}


impl<InnerListener, TMux, TSec> Stream for ListenerUpgrade<InnerListener, TMux, TSec>
where
    InnerListener: TransportListener + Unpin,
    InnerListener::Output: Read2 + Write2 + Unpin + 'static,
    TSec: Upgrader<InnerListener::Output> + 'static,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
    TMux: 'static,
{
    type Item = Result<TMux::Output, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {

        log::error!("start next");

        let mut this = self.project();
        loop {
            let mut made_progress_this_iter = false;

            log::error!("start looping");

            // Check if we've already created a number of futures greater than `limit`
            if this.limit.map(|limit| limit.get() > this.futures.len()).unwrap_or(true) {
                // let poll_res = match stream.as_mut().as_pin_mut() {
                //     Some(stream) => stream.try_poll_next(cx),
                //     None => Poll::Ready(None),
                // };
                let poll_res = this.inner.accept().try_poll_unpin(cx);

                let elem = match poll_res {
                    Poll::Ready(Ok(elem)) => {
                        made_progress_this_iter = true;
                        Some(elem)
                    },
                    // Poll::Ready(None) => {
                    //     stream.set(None);
                    //     None
                    // }
                    Poll::Pending => None,
                    Poll::Ready(Err(e)) => {
                        // Empty the stream and futures so that we know
                        // the future has completed.
                        // stream.set(None);
                        drop(std::mem::replace(this.futures, FuturesUnordered::new()));

                        log::error!("error accepting");


                        return Poll::Ready(None);
                    }
                };

                if let Some(elem) = elem {

                    log::error!("got a raw connection, put onto upgrade future list");

                    let sec = this.sec.clone();
                    let mux = this.mux.clone();
                    this.futures.push(async move {

                        futures_timer::Delay::new(Duration::from_secs(3)).await;

                        let sec_socket = sec.select_inbound(elem).await?;
                        mux.select_outbound(sec_socket).await

                    }.boxed());
                }
            }

            match this.futures.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(s))) => {
                    made_progress_this_iter = true;

                    log::error!("upgraded, return");

                    return Poll::Ready(Some(Ok(s)))
                },
                Poll::Ready(None) => {
                    // if stream.is_none() {
                    //     return Poll::Ready(Ok(()))
                    // }

                    log::error!("error none");
                },
                Poll::Pending => {
                    log::error!("upgrade pending");
                }
                Poll::Ready(Some(Err(e))) => {

                    log::error!("upgrde error, return");

                    // Empty the stream and futures so that we know
                    // the future has completed.
                    // stream.set(None);
                    drop(std::mem::replace(this.futures, FuturesUnordered::new()));
                    return Poll::Ready(Some(Err(e)));
                }
            }

            if !made_progress_this_iter {

                log::error!("error no progress");
                return Poll::Pending;
            }
        }
    }

}

#[async_trait]
impl<InnerTrans, TMux, TSec> Transport for TransportUpgrade<InnerTrans, TMux, TSec>
where
    InnerTrans: Transport,
    InnerTrans::Listener: Unpin,
    InnerTrans::Output: Read2 + Write2 + Unpin + 'static,
    TSec: Upgrader<InnerTrans::Output> + 'static,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
    TMux: 'static
{
    type Output = TMux::Output;
    type Listener = ListenerUpgrade<InnerTrans::Listener, TMux, TSec>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError>
    {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ListenerUpgrade::new(inner_listener, self.mux, self.sec);

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError>
    {
        let socket = self.inner.dial(addr).await?;
        let sec_socket = self.sec.select_outbound(socket).await?;

        self.mux.select_outbound(sec_socket).await
    }
}


impl<InnerListener, TMux, TSec> ListenerUpgrade<InnerListener, TMux, TSec>
    where
        InnerListener: TransportListener + Unpin,
        InnerListener::Output: Read2 + Write2 + Unpin,
        TSec: Upgrader<InnerListener::Output>,
        TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
        TMux: Upgrader<TSec::Output>,
        TMux::Output: StreamMuxer,
{
    pub(crate) fn new(inner: InnerListener, mux: Multistream<TMux>, sec: Multistream<TSec>) -> Self {
        Self {
            inner,
            mux,
            sec,
            futures: FuturesUnordered::new(),
            limit: NonZeroUsize::new(10),
        }
    }
}

#[async_trait]
impl<InnerListener, TMux, TSec> TransportListener for ListenerUpgrade<InnerListener, TMux, TSec>
where
    InnerListener: TransportListener + Unpin,
    InnerListener::Output: Read2 + Write2 + Unpin + 'static,
    TSec: Upgrader<InnerListener::Output> + 'static,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
    TMux: 'static,
{
    type Output = TMux::Output;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {

        self.next().await.unwrap()

    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}

// TODO:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::memory::MemoryTransport;
    use crate::upgrade::dummy::DummyUpgrader;

    #[test]
    fn communicating_between_dialer_and_listener() {

        let msg = [1, 2, 3];

        // Setup listener.
        let rand_port = rand::random::<u64>().saturating_add(1);
        let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();
        let cloned_t1_addr = t1_addr.clone();

        let t1 = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::new(), DummyUpgrader::new());

        let listener = async move {
            let mut listener = t1.listen_on(t1_addr.clone()).unwrap();

            let mut socket = listener.accept().await.unwrap();

            let mut buf = [0; 3];
            socket.read_exact2(&mut buf).await.unwrap();

            assert_eq!(buf, msg);
        };

        // Setup dialer.
        let t2 = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::new(), DummyUpgrader::new());

        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            socket.write_all2(&msg).await.unwrap();
        };

        // Wait for both to finish.

        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}

