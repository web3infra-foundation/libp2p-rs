// Copyright 2018 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

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
            EitherName::A(info) => Ok(EitherOutput::A(self.0.upgrade_outbound(socket, info).await?)),
            EitherName::B(info) => Ok(EitherOutput::B(self.1.upgrade_outbound(socket, info).await?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    //use crate::muxing::StreamMuxer;
    use crate::upgrade::dummy::DummyUpgrader;

    #[test]
    fn verify_basic() {
        let m = Selector::new(DummyUpgrader::new(), DummyUpgrader::new());

        async_std::task::block_on(async move {
            let output = m.upgrade_outbound(100u32, EitherName::A(b"")).await.unwrap();

            let mut _o = match output {
                EitherOutput::A(a) => a,
                EitherOutput::B(a) => a,
            };
            //todo : method not found in `upgrade::dummy::DummyStream<u32>
            //let oo = o.open_stream().await.unwrap();

            //assert_eq!(oo, ());
        });
    }
}
