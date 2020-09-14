use async_trait::async_trait;

use crate::either::{EitherName, EitherOutput};
use crate::transport::TransportError;
use crate::upgrade::{UpgradeInfo, Upgrader};

/// Select two upgrades into one. Supports all the protocols supported by either
/// sub-upgrade.
///
/// The protocols supported by the first element have a higher priority.
#[derive(Debug, Copy, Clone)]
pub struct Selector<A, B>(A, B);

impl<A, B> Selector<A, B> {
    /// Combines two upgraders into an `Selector`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        Selector(a, b)
    }
}

impl<A, B> UpgradeInfo for Selector<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type Info = EitherName<A::Info, B::Info>;

    fn protocol_info(&self) -> Vec<Self::Info> {
        let mut v = Vec::default();
        v.extend(self.0.protocol_info().into_iter().map(EitherName::A));
        v.extend(self.1.protocol_info().into_iter().map(EitherName::B));
        v
    }
}

#[async_trait]
impl<A, B, C> Upgrader<C> for Selector<A, B>
where
    A: Upgrader<C> + Send,
    B: Upgrader<C> + Send,
    C: Send + 'static,
{
    type Output = EitherOutput<A::Output, B::Output>;

    async fn upgrade_inbound(
        self,
        socket: C,
        info: <Self as UpgradeInfo>::Info,
    ) -> Result<EitherOutput<A::Output, B::Output>, TransportError> {
        match info {
            EitherName::A(info) => Ok(EitherOutput::A(self.0.upgrade_inbound(socket, info).await?)),
            EitherName::B(info) => Ok(EitherOutput::B(self.1.upgrade_inbound(socket, info).await?)),
        }
    }

    async fn upgrade_outbound(
        self,
        socket: C,
        info: <Self as UpgradeInfo>::Info,
    ) -> Result<EitherOutput<A::Output, B::Output>, TransportError> {
        match info {
            EitherName::A(info) => Ok(EitherOutput::A(
                self.0.upgrade_outbound(socket, info).await?,
            )),
            EitherName::B(info) => Ok(EitherOutput::B(
                self.1.upgrade_outbound(socket, info).await?,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::muxing::StreamMuxer;
    use crate::upgrade::dummy::DummyUpgrader;

    #[test]
    fn verify_basic() {
        let m = Selector::new(DummyUpgrader::new(), DummyUpgrader::new());

        async_std::task::block_on(async move {
            let output = m
                .upgrade_outbound(100u32, EitherName::A(b""))
                .await
                .unwrap();

            let mut o = match output {
                EitherOutput::A(a) => a,
                EitherOutput::B(a) => a,
            };

            let oo = o.open_stream().await.unwrap();

            assert_eq!(oo, ());
        });
    }
}
