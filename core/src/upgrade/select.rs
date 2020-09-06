
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
#[derive(Debug, Clone)]
pub struct MultistreamSelector<A, B>(A, B);

impl<A, B> MultistreamSelector<A, B> {
    /// Combines two upgraders into an `MultistreamSelector`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        MultistreamSelector(a, b)
    }

    fn protocol_info(&self) -> InfoIterChain<<A::InfoIter as IntoIterator>::IntoIter, <B::InfoIter as IntoIterator>::IntoIter>
        where
            A: UpgradeInfo,
            B: UpgradeInfo
    {
        InfoIterChain(self.0.protocol_info().into_iter(), self.1.protocol_info().into_iter())
    }

    async fn select_inbound<C>(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
        where
            A: Upgrader<C>,
            B: Upgrader<C>,
            C: Read2 + Write2
    {
        // perform multi-stream selection to get the protocol we are going to run
        // TODO: multi stream
        let _protocols = self.protocol_info();
        let a = self.0.protocol_info().into_iter().next().unwrap();
        let info = EitherName::<A::Info, B::Info>::A(a);

        match info {
            EitherName::A(info) => Ok(EitherOutput::A(self.0.upgrade_inbound(socket).await?)),
            EitherName::B(info) => Ok(EitherOutput::B(self.1.upgrade_inbound(socket).await?)),
        }
    }

    async fn select_outbound<C>(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
        where
            A: Upgrader<C>,
            B: Upgrader<C>,
            C: Read2 + Write2
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


/// Iterator that combines the protocol names of twp upgrades.
#[derive(Debug, Clone)]
pub struct InfoIterChain<A, B>(A, B);

impl<A, B> Iterator for InfoIterChain<A, B>
where
    A: Iterator,
    B: Iterator
{
    type Item = EitherName<A::Item, B::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(info) = self.0.next() {
            return Some(EitherName::A(info))
        }
        if let Some(info) = self.1.next() {
            return Some(EitherName::B(info))
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min1, max1) = self.0.size_hint();
        let (min2, max2) = self.1.size_hint();
        let max = max1.and_then(move |m1| max2.and_then(move |m2| m1.checked_add(m2)));
        (min1.saturating_add(min2), max)
    }
}

