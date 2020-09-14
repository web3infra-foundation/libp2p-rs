//! Transport upgrader.
//!
// TODO: add example

use crate::either::EitherOutput;
use crate::muxing::StreamMuxer;
use crate::secure_io::SecureInfo;
use crate::transport::TransportListener;
use crate::upgrade::multistream::Multistream;
use crate::upgrade::Upgrader;
use crate::{transport::TransportError, Multiaddr, Transport};
use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};
use log::trace;
use pnet::Pnet;

/// A `TransportUpgrade` is a `Transport` that wraps another `Transport` and adds
/// upgrade capabilities to all inbound and outbound connection attempts.
///
#[derive(Debug, Clone)]
pub struct TransportUpgrade<InnerTrans, TProtector, TMux, TSec> {
    inner: InnerTrans,
    pnet: TProtector,
    // protector: Option<TProtector>,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
}

impl<InnerTrans, TProtector, TMux, TSec> TransportUpgrade<InnerTrans, TProtector, TMux, TSec>
where
    InnerTrans: Transport,
    InnerTrans::Output: Read2 + Write2 + Unpin,
    TProtector: Pnet<InnerTrans::Output> + Send + Clone,
    TProtector::Output: Read2 + Write2 + Unpin,
    TSec: Upgrader<EitherOutput<TProtector::Output, InnerTrans::Output>> + Send + Clone,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
{
    /// Wraps around a `Transport` to add upgrade capabilities.
    pub fn new(inner: InnerTrans, pnet: TProtector, mux: TMux, sec: TSec) -> Self {
        TransportUpgrade {
            inner,
            pnet,
            sec: Multistream::new(sec),
            mux: Multistream::new(mux),
        }
    }
}

#[async_trait]
impl<InnerTrans, TProtector, TMux, TSec> Transport
    for TransportUpgrade<InnerTrans, TProtector, TMux, TSec>
where
    InnerTrans: Transport,
    InnerTrans::Output: Read2 + Write2 + Unpin,
    TProtector: Pnet<InnerTrans::Output> + Send + Clone + Sync,
    TProtector::Output: Read2 + Write2 + Unpin,
    TSec: Upgrader<EitherOutput<TProtector::Output, InnerTrans::Output>> + Send + Clone,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
{
    type Output = TMux::Output;
    type Listener = ListenerUpgrade<InnerTrans::Listener, TProtector, TMux, TSec>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ListenerUpgrade::new(inner_listener, self.pnet, self.mux, self.sec);

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket = self.inner.dial(addr).await?;
        let maybe_encrypted = match &self.pnet.has_key() {
            true => match self.pnet.handshake(socket).await {
                Ok(output) => EitherOutput::A(output),
                Err(_) => return Err(TransportError::Internal),
            },
            false => EitherOutput::B(socket),
        };
        let sec_socket = self.sec.select_outbound(maybe_encrypted).await?;

        self.mux.select_outbound(sec_socket).await
    }
}
pub struct ListenerUpgrade<InnerListener, TProtector, TMux, TSec> {
    inner: InnerListener,
    pnet: TProtector,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
    // TODO: add threshold support here
}

impl<InnerListener, TProtector, TMux, TSec> ListenerUpgrade<InnerListener, TProtector, TMux, TSec> {
    pub(crate) fn new(
        inner: InnerListener,
        pnet: TProtector,
        mux: Multistream<TMux>,
        sec: Multistream<TSec>,
    ) -> Self {
        Self {
            inner,
            pnet,
            mux,
            sec,
        }
    }
}

#[async_trait]
impl<InnerListener, TProtector, TMux, TSec> TransportListener
    for ListenerUpgrade<InnerListener, TProtector, TMux, TSec>
where
    InnerListener: TransportListener,
    InnerListener::Output: Read2 + Write2 + Unpin,
    TProtector: Pnet<InnerListener::Output> + Send + Clone + Sync,
    TProtector::Output: Read2 + Write2 + Unpin,
    TSec: Upgrader<EitherOutput<TProtector::Output, InnerListener::Output>> + Send + Clone,
    TSec::Output: SecureInfo + Read2 + Write2 + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
{
    type Output = TMux::Output;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let stream = self.inner.accept().await?;
        let maybe_encrypted = match &self.pnet.has_key() {
            true => match self.pnet.clone().handshake(stream).await {
                Ok(output) => EitherOutput::A(output),
                Err(_) => return Err(TransportError::HandshakeError),
            },
            false => EitherOutput::B(stream),
        };
        let sec = self.sec.clone();

        trace!("got a new connection, upgrading...");
        //futures_timer::Delay::new(Duration::from_secs(3)).await;
        let sec_socket = sec.select_inbound(maybe_encrypted).await?;

        let mux = self.mux.clone();

        mux.select_inbound(sec_socket).await
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
    fn communicating_between_dialer_and_listener() {
        let msg = [1, 2, 3];

        // Setup listener.
        let rand_port = rand::random::<u64>().saturating_add(1);
        let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();
        let cloned_t1_addr = t1_addr.clone();
        let psk = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683".parse::<PreSharedKey>().unwrap();
        let pnet = PnetConfig::new(Some(psk));
        let t1 = TransportUpgrade::new(
            MemoryTransport::default(),
            pnet,
            DummyUpgrader::new(),
            DummyUpgrader::new(),
        );

        let listener = async move {
            let mut listener = t1.listen_on(t1_addr.clone()).unwrap();

            let mut socket = listener.accept().await.unwrap();

            let mut buf = [0; 3];
            socket.read_exact2(&mut buf).await.unwrap();

            assert_eq!(buf, msg);
        };

        // Setup dialer.
        let t2 = TransportUpgrade::new(
            MemoryTransport::default(),
            pnet,
            DummyUpgrader::new(),
            DummyUpgrader::new(),
        );

        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            socket.write_all2(&msg).await.unwrap();
        };

        // Wait for both to finish.

        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}
