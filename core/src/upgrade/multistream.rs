
use async_trait::async_trait;
use crate::transport::TransportError;
use std::iter;
use crate::upgrade::{UpgradeInfo, Upgrader};
use std::collections::HashMap;


type BoxedUpgrader = Box<dyn Upgrader<T>>;

/// Upgrade that uses multistream-selector to select protocols.
///
///
#[derive(Debug, Clone)]
pub struct MultistreamSelector
{
    handlers: HashMap<String, BoxedUpgrader>,
}

impl MultistreamSelector {
    /// Combines two upgrades into an `MultistreamSelector`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new() -> Self
    {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn add_handler(&mut self, up: BoxedUpgrader) {
        self.handlers.insert(up.protocol_info().into_iter().next().unwrap(), up);
    }
}

#[async_trait]
impl UpgradeInfo for MultistreamSelector
{
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/multistream/1.0.0")
    }
}

#[async_trait]
impl<C> Upgrader<C> for MultistreamSelector
where
    C: Send + 'static
{
    type Output = BoxedUpgrader;

    async fn upgrade_inbound(self, socket: C) -> Result<Self::Output, TransportError> {
        //TODO: multi stream select ...

        let mut h = self.handlers;

        let t = h.remove("default".parse().unwrap()).unwrap();
        t.clone().upgrade_inbound(socket).await
    }

    async fn upgrade_outbound(self, socket: C) -> Result<Self::Output, TransportError> {
        let mut h = self.handlers;

        let t = h.remove("default".parse().unwrap()).unwrap();
        t.clone().upgrade_outbound(socket).await
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

        let dummy = dummy.and_then(DummyUpgrader::new());

        //let s = dummy.upgrade_inbound(8);


    }
}
