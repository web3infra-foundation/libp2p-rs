
//! Transport upgrader.
//!
// TODO: add example

use async_trait::async_trait;
use futures_timer::Delay;
use futures::{StreamExt, Stream, SinkExt, TryStreamExt};
use futures::prelude::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{time::Duration};
use libp2p_traits::Write2;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::select;
use log::{trace};
use crate::{Multiaddr, Transport, transport::{TransportError}};
use crate::transport::TransportListener;
use crate::upgrade::Upgrader;

//use crate::transport::security::SecurityUpgrader;


/// A `TransportUpgrade` is a `Transport` that wraps another `Transport` and adds
/// upgrade capabilities to all inbound and outbound connection attempts.
///
#[derive(Debug, Copy, Clone)]
pub struct TransportUpgrade<InnerTrans, S> {
    inner: InnerTrans,

    // protector: Option<TProtector>,
    up: S,
    // mux_up: Option<TMuxUpgrader>,
}

impl<InnerTrans, S> TransportUpgrade<InnerTrans, S>
where
    InnerTrans: Transport + Send,
    S: Upgrader<InnerTrans::Output>
{
    /// Wraps around a `Transport` to add upgrade capabilities.
    pub fn new(inner: InnerTrans, up: S) -> Self {
        TransportUpgrade {
            inner,
            up
        }
    }

/*    /// Upgrades the transport to setup the private network.
    ///
    pub fn protect(mut self, protector: TProtector) -> Self
    {
        self.protector = Some(protector);
        self
    }

    /// Upgrades the transport to setup the security.
    ///
    pub fn secure(mut self, secure_up: TSecurityUpgrader) -> Self
    where
        TSecurityUpgrader: Upgrader<InnerTrans::Output, Output = InnerTrans::Output>,
    {
        self.secure_up = Some(secure_up);
        self
    }
*/}

#[async_trait]
impl<InnerTrans, S> Transport for TransportUpgrade<InnerTrans, S>
where
    InnerTrans: Transport + Send + Clone,
    S: Upgrader<InnerTrans::Output> + Send + Sync,
{
    type Output = S::Output;
    type Listener = ListenerUpgrade<InnerTrans::Listener, S>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {

        let listener = self.inner.listen_on(addr)?;
        //let (mut tx, mut rx) = mpsc::channel(10);

        let listener = ListenerUpgrade {
            inner: listener,
            up: self.up

            // tx,
        };

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError>
    {
        let mut stream = self.inner.dial(addr).await?;

        //stream.write2(b"hello").await?;

        let mut s = self.up.upgrade_outbound(stream).await?;

        //s.write(b"hello").await?;
        Ok(s)
    }
}

pub struct ListenerUpgrade<InnerListener, S> {
    inner: InnerListener,
    //rx: mpsc::Receiver<T>,
    //tx: mpsc::Receiver<T>,

    up: S,
    // TODO: add threshold support here
}

impl<InnerListener, S> ListenerUpgrade<InnerListener, S> {
    pub fn incoming(&mut self) -> Incoming<'_, InnerListener, S> {
        Incoming(self)
    }
}

pub struct Incoming<'a, InnerListener, S>(&'a mut ListenerUpgrade<InnerListener, S>);

impl<'a, InnerListener, S> Stream for Incoming<'a, InnerListener, S>
where
    InnerListener: TransportListener
{
    type Item = Result<InnerListener::Output, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let future = self.0.inner.accept();
        futures::pin_mut!(future);

        let socket = futures::ready!(future.poll(cx))?;
        Poll::Ready(Some(Ok(socket)))
    }
}

#[async_trait]
impl<InnerListener, S> TransportListener for ListenerUpgrade<InnerListener, S>
where
    InnerListener: TransportListener + Send + Sync,
    S: Upgrader<InnerListener::Output> + Send + Sync
{
    type Output = S::Output;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {

        let mut stream = self.inner.accept().await?;

        trace!("got a new connection, upgrading...");

        let ss = self.up.upgrade_inbound(stream).await?;
        Ok(ss)




        // let mut cc = self.incoming().try_for_each_concurrent(20,|s| {let mut tx = tx.clone(); async move {
        //     trace!("connected first");
        //     futures_timer::Delay::new(Duration::from_secs(3)).await;
        //     tx.send(s).await;
        //     trace!("send an upgrade");
        //     Ok(())
        // }});
        //
        // loop {
        //     let _ = select! {
        //         // // stream = self.inner.accept().fuse() => {
        //         // //     trace!("connected first");
        //         // //     let mut stream = stream?;
        //         // //     //stream.write2(b"upgrading").await?;
        //         // //     trace!("send an upgrade");
        //         // //     tx.send(async {stream}).await;
        //         // // },
        //         _c = cc => {},
        //         up = rx.next() => {
        //             trace!("got an upgrade");
        //
        //             let mut up = up.ok_or(TransportError::Internal)?;
        //
        //
        //             trace!("upgrade ready to go");
        //
        //             //let mut s = self.security_muxer.upgrade_outbound(up).await.unwrap();
        //             //up.write2(b"ready").await?;
        //
        //             return Ok(up);
        //         }
        //     };
        //
        //
        //
        //     // if let Some(up) = r {
        //     //     let mut s = self.accept().await.unwrap();
        //     //     return Ok(s);
        //     // }
        // }

    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::memory::MemoryTransport;
    use crate::upgrade::dummy::DummyUpgrader;


    #[test]
    fn listen() {
        let transport = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::new());
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

        let t1 = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::new());

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

        let t2 = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::new());

        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            socket.write_all(&msg).await.unwrap();
        };

        // Wait for both to finish.

        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}

