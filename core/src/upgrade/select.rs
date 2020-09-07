
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
pub struct MultistreamSelector<A, B>(A, B);

impl<A, B> MultistreamSelector<A, B> {
    /// Combines two upgraders into an `MultistreamSelector`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        MultistreamSelector(a, b)
    }

    fn protocol_info(&self) -> InfoIterChain<Vec<A::Info>, Vec<B::Info>>
        where
            A: UpgradeInfo,
            B: UpgradeInfo,
    {
        InfoIterChain(self.0.protocol_info(), self.1.protocol_info())
    }

    pub async fn select_inbound<C>(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
        where
            A: Upgrader<C> + Send,
            B: Upgrader<C> + Send,
            // A::InfoIter: Send,
            // B::InfoIter: Send,
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

    pub async fn select_outbound<C>(self, socket: C) -> Result<EitherOutput<A::Output, B::Output>, TransportError>
        where
            A: Upgrader<C> + Send,
            B: Upgrader<C> + Send,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::dummy::DummyUpgrader;
    use crate::transport::memory::MemoryTransport;
    use crate::{Transport, Multiaddr};


    #[test]
    fn verify_basic() {

        struct TTT<A,B>(MultistreamSelector<A, B>);

        //#[async_trait]
        impl<A, B> TTT<A,B>
            where
                A: Upgrader<u32> + Send,
                B: Upgrader<u32> + Send,
        {
            // type Output = ();
            // type Listener = ();

            // fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> where
            //     Self: Sized {
            //     unimplemented!()
            // }

            async fn dial(self, _addr: Multiaddr) -> Result<(), TransportError>
            {
                self.0.select_outbound(100).await;

                Ok(())
            }
        }

        let transport = MemoryTransport::default();

        let m = MultistreamSelector::new(DummyUpgrader::new(), DummyUpgrader::new());

        let ttt = TTT(m);



        async_std::task::spawn(async move {

            let st = transport.dial("/memory/12345".parse().unwrap()).await.unwrap();

            ttt.dial("/memory/12345".parse().unwrap()).await;


            // let o = match s {
            //     EitherOutput::A(info) => info,
            //     EitherOutput::B(info) => info,
            // };



            //assert_eq!(o, 100);
        });
    }
}
