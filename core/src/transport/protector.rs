use crate::pnet::{Pnet, PnetConfig, PnetOutput};
use crate::transport::{ConnectionInfo, IListener};
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

    fn listen_on(self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        let inner_listener = self.inner.listen_on(addr)?;
        let listener = ProtectorListener::new(inner_listener, self.pnet);
        Ok(Box::new(listener))
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket = self.inner.dial(addr).await?;
        self.pnet.handshake(socket).await.map_err(|e| e.into())
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
    TOutput: ConnectionInfo + ReadEx + WriteEx + Unpin + 'static,
{
    type Output = PnetOutput<TOutput>;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let stream = self.inner.accept().await?;
        self.pnet.clone().handshake(stream).await.map_err(|e| e.into())
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}
