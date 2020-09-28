use crate::pnet::{Pnet, PnetConfig, PnetOutput};
use crate::transport::ConnectionInfo;
use crate::{
    transport::{TransportError, TransportListener},
    Multiaddr, Transport,
};
use async_trait::async_trait;
use libp2p_traits::{ReadEx, WriteEx};

#[derive(Debug, Copy, Clone)]
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
    InnerTrans: Transport,
    InnerTrans::Output: ConnectionInfo + ReadEx + WriteEx + Unpin + 'static,
{
    type Output = PnetOutput<InnerTrans::Output>;
    type Listener = ProtectorListener<InnerTrans::Listener>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ProtectorListener::new(inner_listener, self.pnet);
        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket = self.inner.dial(addr).await?;
        self.pnet.handshake(socket).await.map_err(|e|e.into())
    }
}

pub struct ProtectorListener<InnerListener> {
    inner: InnerListener,
    pnet: PnetConfig,
}

impl<InnerListener> ProtectorListener<InnerListener> {
    pub(crate) fn new(inner: InnerListener, pnet: PnetConfig) -> Self {
        Self { inner, pnet }
    }
}

#[async_trait]
impl<InnerListener> TransportListener for ProtectorListener<InnerListener>
where
    InnerListener: TransportListener,
    InnerListener::Output: ConnectionInfo + ReadEx + WriteEx + Unpin + 'static,
{
    type Output = PnetOutput<InnerListener::Output>;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let stream = self.inner.accept().await?;
        self.pnet.clone().handshake(stream).await.map_err(|e|e.into())
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}
