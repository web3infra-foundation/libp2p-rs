

use crate::{
    either::{EitherOutput, EitherError, EitherFuture2, EitherName},
    upgrade::{Upgrader, UpgradeInfo}
};
use crate::transport::TransportError;

/// Upgrade that combines two upgrades into one. Supports all the protocols supported by either
/// sub-upgrade.
///
/// The protocols supported by the first element have a higher priority.
#[derive(Debug, Clone)]
pub struct SelectUpgrade<A, B>(A, B);

impl<A, B> SelectUpgrade<A, B> {
    /// Combines two upgrades into an `SelectUpgrade`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        SelectUpgrade(a, b)
    }
}

impl<A, B> UpgradeInfo for SelectUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo
{
    type Info = EitherName<A::Info, B::Info>;
    type InfoIter = InfoIterChain<
        <A::InfoIter as IntoIterator>::IntoIter,
        <B::InfoIter as IntoIterator>::IntoIter
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        InfoIterChain(self.0.protocol_info().into_iter(), self.1.protocol_info().into_iter())
    }
}

impl<C, A, B, TA, TB, EA, EB> Upgrader<C> for SelectUpgrade<A, B>
where
    A: InboundUpgrade<C, Output = TA, Error = EA>,
    B: InboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = EitherOutput<TA, TB>;

    async fn upgrade_inbound(self, socket: C, info: Self::Info) -> Self::Future {
        match info {
            EitherName::A(info) => EitherFuture2::A(self.0.upgrade_inbound(sock, info)),
            EitherName::B(info) => EitherFuture2::B(self.1.upgrade_inbound(sock, info))
        }
    }

    async fn upgrade_outbound(self, socket: C) -> Result<Self::Output, TransportError> {
        unimplemented!()
    }
}

impl<C, A, B, TA, TB, EA, EB> OutboundUpgrade<C> for SelectUpgrade<A, B>
where
    A: OutboundUpgrade<C, Output = TA, Error = EA>,
    B: OutboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = EitherOutput<TA, TB>;
    type Error = EitherError<EA, EB>;
    type Future = EitherFuture2<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        match info {
            EitherName::A(info) => EitherFuture2::A(self.0.upgrade_outbound(sock, info)),
            EitherName::B(info) => EitherFuture2::B(self.1.upgrade_outbound(sock, info))
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

