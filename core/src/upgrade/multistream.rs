use crate::multistream::Negotiator;
use crate::transport::TransportError;
use crate::upgrade::{ProtocolName, Upgrader};
use libp2p_traits::{Read2 as ReadEx, Write2 as WriteEx};
use log::{info, trace};

//b"/multistream/1.0.0"

/// Multistream that uses multistream-select to select protocols.
///
///
#[derive(Debug, Clone)]
pub(crate) struct Multistream<U> {
    inner: U,
}

impl<U> Multistream<U> {
    /// Add `Multistream` on top of any `Upgrader`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(inner: U) -> Self {
        Self { inner }
    }
}

impl<U> Multistream<U> {
    pub(crate) async fn select_inbound<C>(self, socket: C) -> Result<U::Output, TransportError>
    where
        C: ReadEx + WriteEx + Send + Unpin,
        U: Upgrader<C> + Send,
    {
        trace!("starting multistream select for inbound...");
        let protocols = self.inner.protocol_info();
        let neg = Negotiator::new_with_protocols(
            protocols.into_iter().map(NameWrap as fn(_) -> NameWrap<_>),
        );

        let (proto, socket) = neg.negotiate(socket).await?;

        info!("select_inbound {:?}", proto.protocol_name_str());
        self.inner.upgrade_inbound(socket, proto.0).await
    }

    pub(crate) async fn select_outbound<C: Send + Unpin>(
        self,
        socket: C,
    ) -> Result<U::Output, TransportError>
    where
        C: ReadEx + WriteEx + Send + Unpin,
        U: Upgrader<C> + Send,
    {
        trace!("starting multistream select for outbound...");
        let protocols = self.inner.protocol_info();
        let neg = Negotiator::new_with_protocols(
            protocols.into_iter().map(NameWrap as fn(_) -> NameWrap<_>),
        );

        let (proto, socket) = neg.select_one(socket).await?;

        info!("select_outbound {:?}", proto.protocol_name_str());
        self.inner.upgrade_outbound(socket, proto.0).await
    }
}

#[derive(Clone)]
struct NameWrap<N>(N);

impl<N: ProtocolName> AsRef<[u8]> for NameWrap<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.protocol_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::DummyUpgrader;

    #[test]
    fn to_be_done() {}
}
