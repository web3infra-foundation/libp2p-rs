
use log::{trace};
use crate::transport::TransportError;
use crate::upgrade::{Upgrader, ProtocolName};

//b"/multistream/1.0.0"

/// Multistream that uses multistream-select to select protocols.
///
///
#[derive(Debug, Clone)]
pub(crate) struct Multistream<U>
{
    inner: U,
}

impl<U> Multistream<U> {
    /// Add `Multistream` on top of any `Upgrader`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(inner: U) -> Self
    {
        Self {
            inner,
        }
    }
}

impl<U> Multistream<U>
{
    pub(crate) async fn select_inbound<C>(self, socket: C) -> Result<U::Output, TransportError>
        where
            U: Upgrader<C> + Send
    {
        trace!("starting multistream select for inbound...");
        //TODO: multi stream select ...
        let protocols = self.inner.protocol_info();
        let a = protocols.into_iter().next().unwrap();

        log::info!("select_inbound {:?}", a.protocol_name_str());
        self.inner.upgrade_inbound(socket, a).await
    }

    pub(crate) async fn select_outbound<C>(self, socket: C) -> Result<U::Output, TransportError>
        where
            U: Upgrader<C> + Send
    {
        trace!("starting multistream select for outbound...");
        //TODO: multi stream select ...
        let protocols = self.inner.protocol_info();
        let a = protocols.into_iter().next().unwrap();

        log::info!("select_outbound {:?}", a.protocol_name_str());
        self.inner.upgrade_outbound(socket, a).await
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::{DummyUpgrader};

    #[test]
    fn and_then() {

        let dummy = DummyUpgrader::new();
        let n = dummy.protocol_info();

        //let dummy = dummy.and_then(DummyUpgrader::new());

        //let s = dummy.upgrade_inbound(8);


    }
}
