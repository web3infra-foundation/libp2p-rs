
use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};

use crate::{
    upgrade::{Upgrader, UpgradeInfo}
};
use crate::transport::TransportError;
use crate::either::{EitherOutput, EitherName};

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

    fn protocol_info(&self) -> Vec<Self::Info>
    {
        let mut v = Vec::default();
        v.extend(self.0.protocol_info().into_iter().map(|a| EitherName::A(a)));
        v.extend(self.1.protocol_info().into_iter().map(|a| EitherName::B(a)));
        v
    }
}

#[async_trait]
impl<A, B, C> Upgrader<C> for Selector<A, B>
    where
        A: Upgrader<C> + Send,
        B: Upgrader<C> + Send,
        C: Send + 'static
{
    type Output = EitherOutput<A::Output, B::Output>;

    async fn upgrade_inbound(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
    {
        let _protocols = self.protocol_info();
        let a = self.0.protocol_info().into_iter().next().unwrap();
        let info = EitherName::<A::Info, B::Info>::A(a);

        match info {
            EitherName::A(info) => Ok(EitherOutput::A(self.0.upgrade_inbound(socket).await?)),
            EitherName::B(info) => Ok(EitherOutput::B(self.1.upgrade_inbound(socket).await?)),
        }
    }

    async fn upgrade_outbound(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
    {
        // perform multi-stream selection to get the protocol we are going to run
        // TODO: multi stream
        let _protocols = self.protocol_info();
        let a = self.0.protocol_info().into_iter().next().unwrap();
        let info = EitherName::<A::Info, B::Info>::A(a);

        match info {
            EitherName::A(info) => Ok(EitherOutput::A(self.0.upgrade_outbound(socket).await?)),
            EitherName::B(info) => Ok(EitherOutput::B(self.1.upgrade_outbound(socket).await?)),
        }
    }
}
//
//
// /// Iterator that combines the protocol names of twp upgrades.
// #[derive(Debug, Clone)]
// pub struct InfoIterChain<A, B>(A, B);
//
// impl<A, B> Iterator for InfoIterChain<A, B>
// where
//     A: Iterator,
//     B: Iterator
// {
//     type Item = EitherName<A::Item, B::Item>;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         if let Some(info) = self.0.next() {
//             return Some(EitherName::A(info))
//         }
//         if let Some(info) = self.1.next() {
//             return Some(EitherName::B(info))
//         }
//         None
//     }
//
//     fn size_hint(&self) -> (usize, Option<usize>) {
//         let (min1, max1) = self.0.size_hint();
//         let (min2, max2) = self.1.size_hint();
//         let max = max1.and_then(move |m1| max2.and_then(move |m2| m1.checked_add(m2)));
//         (min1.saturating_add(min2), max)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::dummy::DummyUpgrader;

    #[test]
    fn verify_basic() {

        let m = Selector::new(DummyUpgrader::new(), DummyUpgrader::new());

        async_std::task::block_on(async move {
            let output = m.upgrade_outbound(100).await.unwrap();

            let o = match output {
                EitherOutput::A(a) => a,
                EitherOutput::B(a) => a,
            };

            assert_eq!(o, 100);
        });
    }
}
