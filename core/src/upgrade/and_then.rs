
use async_trait::async_trait;
use crate::{
    upgrade::Upgrader
};
use crate::transport::TransportError;
use std::iter;
use crate::upgrade::UpgradeInfo;

/// Upgrade that combines all upgrades into one. Supports all the protocols supported by either
/// sub-upgrade.
///
///
#[derive(Debug, Clone)]
pub struct AndThenUpgrader<A, B>
{
    first: A,
    second: B,
}

impl<A, B> AndThenUpgrader<A, B> {
    /// Combines two upgrades into an `AndThenUpgrader`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(first: A, second: B) -> Self
    where
        //A: Upgrader<T>,
        //B: Upgrader<A::Output>,
    {
        Self {
            first,
            second,
        }
    }
}

impl<A, B> UpgradeInfo for AndThenUpgrader<A, B>
{
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/stack/1.0.0"]
    }
}

#[async_trait]
impl<A, B, C> Upgrader<C> for AndThenUpgrader<A, B>
where
    A: Upgrader<C> + Send,
    B: Upgrader<A::Output> + Send,
    C: Send + 'static
{
    type Output = B::Output;

    async fn upgrade_inbound(self, socket: C) -> Result<Self::Output, TransportError> {
        let s = self.first.upgrade_inbound(socket).await?;
        self.second.upgrade_inbound(s).await
    }

    async fn upgrade_outbound(self, socket: C) -> Result<Self::Output, TransportError> {
        let s = self.first.upgrade_outbound(socket).await?;
        self.second.upgrade_outbound(s).await
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::{DummyUpgrader};

    // #[test]
    // fn and_then() {
    //
    //     let dummy = DummyUpgrader::new();
    //     let n = dummy.protocol_info();
    //
    //     let dummy = dummy.and_then(DummyUpgrader::new());
    //
    //     //let s = dummy.upgrade_inbound(8);
    //
    //
    // }
}
