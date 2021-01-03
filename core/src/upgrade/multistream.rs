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

use crate::multistream::Negotiator;
use crate::transport::TransportError;
use crate::upgrade::{ProtocolName, Upgrader};
use libp2prs_traits::{ReadEx, WriteEx};
use log::{debug, trace};

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
        C: ReadEx + WriteEx + Unpin,
        U: Upgrader<C> + Send,
    {
        trace!("starting multistream select for inbound...");
        let protocols = self.inner.protocol_info();
        let neg = Negotiator::new_with_protocols(protocols.into_iter().map(NameWrap as fn(_) -> NameWrap<_>));

        let (proto, socket) = neg.negotiate(socket).await?;

        debug!("select_inbound {:?}", proto);
        self.inner.upgrade_inbound(socket, proto.0).await
    }

    pub(crate) async fn select_outbound<C: Send + Unpin>(self, socket: C) -> Result<U::Output, TransportError>
    where
        C: ReadEx + WriteEx + Unpin,
        U: Upgrader<C> + Send,
    {
        trace!("starting multistream select for outbound...");
        let protocols = self.inner.protocol_info();
        let neg = Negotiator::new_with_protocols(protocols.into_iter().map(NameWrap as fn(_) -> NameWrap<_>));

        let (proto, socket) = neg.select_one(socket).await?;

        debug!("select_outbound {:?}", proto);
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

impl<N: ProtocolName> std::fmt::Debug for NameWrap<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.0.protocol_name()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn to_be_done() {}
}
