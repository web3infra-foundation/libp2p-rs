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

//! Transport upgrader.
//!
// TODO: add example

use crate::muxing::{IStreamMuxer, StreamMuxer, StreamMuxerEx};
use crate::secure_io::SecureInfo;
use crate::transport::{ConnectionInfo, IListener, ITransport, TransportListener};
use crate::upgrade::multistream::Multistream;
use crate::upgrade::Upgrader;
use crate::{transport::TransportError, Multiaddr, Transport};
use async_trait::async_trait;
use libp2prs_traits::{ReadEx, WriteEx};
use log::trace;

/// A `TransportUpgrade` is a `Transport` that wraps another `Transport` and adds
/// upgrade capabilities to all inbound and outbound connection attempts.
///
#[derive(Debug, Clone)]
pub struct TransportUpgrade<InnerTrans, TMux, TSec> {
    inner: InnerTrans,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
}

impl<InnerTrans, TMux, TSec> TransportUpgrade<InnerTrans, TMux, TSec>
where
    InnerTrans: Transport,
    InnerTrans::Output: ConnectionInfo + ReadEx + WriteEx + Unpin,
    TSec: Upgrader<InnerTrans::Output>,
    TSec::Output: SecureInfo + ReadEx + WriteEx + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
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

#[async_trait]
impl<InnerTrans, TMux, TSec> Transport for TransportUpgrade<InnerTrans, TMux, TSec>
where
    InnerTrans: Transport + Clone + 'static,
    InnerTrans::Output: ConnectionInfo + ReadEx + WriteEx + Unpin + 'static,
    TSec: Upgrader<InnerTrans::Output> + 'static,
    TSec::Output: SecureInfo + ReadEx + WriteEx + Unpin,
    TMux: Upgrader<TSec::Output> + 'static,
    TMux::Output: StreamMuxerEx + 'static,
{
    type Output = IStreamMuxer;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ListenerUpgrade::new(inner_listener, self.mux.clone(), self.sec.clone());

        Ok(Box::new(listener))
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        trace!("dialing {} ...", addr);
        let socket = self.inner.dial(addr).await?;
        let sec = self.sec.clone();
        let sec_socket = sec.select_outbound(socket).await?;
        let mux = self.mux.clone();
        let o = mux.select_outbound(sec_socket).await?;
        Ok(Box::new(o))
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        self.inner.protocols()
    }
}
pub struct ListenerUpgrade<TOutput, TMux, TSec> {
    inner: IListener<TOutput>,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
    // TODO: add threshold support here
}

impl<TOutput, TMux, TSec> ListenerUpgrade<TOutput, TMux, TSec> {
    pub(crate) fn new(inner: IListener<TOutput>, mux: Multistream<TMux>, sec: Multistream<TSec>) -> Self {
        Self { inner, mux, sec }
    }
}

#[async_trait]
impl<TOutput, TMux, TSec> TransportListener for ListenerUpgrade<TOutput, TMux, TSec>
where
    TOutput: ConnectionInfo + ReadEx + WriteEx + Unpin,
    TSec: Upgrader<TOutput> + Send + Clone,
    TSec::Output: SecureInfo + ReadEx + WriteEx + Unpin,
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxerEx + 'static,
{
    type Output = IStreamMuxer;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let stream = self.inner.accept().await?;
        let sec = self.sec.clone();

        trace!("accept a new connection from {}, upgrading...", stream.remote_multiaddr());
        //futures_timer::Delay::new(Duration::from_secs(3)).await;
        let sec_socket = sec.select_inbound(stream).await?;

        let mux = self.mux.clone();

        let o = mux.select_inbound(sec_socket).await?;
        Ok(Box::new(o))
    }

    fn multi_addr(&self) -> Vec<Multiaddr> {
        self.inner.multi_addr()
    }
}

/// Trait object for TransportListener which is actually ListenerUpgrade
pub type IListenerEx = IListener<IStreamMuxer>;
/// Trait object for Transport which is actually TransportUpgrade
pub type ITransportEx = ITransport<IStreamMuxer>;

impl Clone for ITransportEx {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pnet::*;
    use crate::transport::memory::MemoryTransport;
    use crate::transport::protector::ProtectorTransport;
    use crate::upgrade::dummy::DummyUpgrader;
    #[test]
    fn test_dialer_and_listener() {
        // Setup listener.
        let rand_port = rand::random::<u64>().saturating_add(1);
        let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();
        let cloned_t1_addr = t1_addr.clone();
        let psk = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683"
            .parse::<PreSharedKey>()
            .unwrap();
        let pnet = PnetConfig::new(psk);
        let pro_trans = ProtectorTransport::new(MemoryTransport::default(), pnet);
        let mut t1 = TransportUpgrade::new(pro_trans.clone(), DummyUpgrader::new(), DummyUpgrader::new());

        let listener = async move {
            let mut listener = t1.listen_on(t1_addr.clone()).unwrap();

            let mut socket = listener.accept().await.unwrap();

            let r = socket.accept_stream().await;

            assert!(r.is_err());
        };

        // Setup dialer.
        let mut t2 = TransportUpgrade::new(pro_trans, DummyUpgrader::new(), DummyUpgrader::new());

        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            let r = socket.open_stream().await;

            assert!(r.is_err());
        };

        // Wait for both to finish.
        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}
