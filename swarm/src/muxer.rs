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

use fnv::FnvHashMap;
use libp2prs_core::multistream::Negotiator;
use libp2prs_core::muxing::IReadWrite;
use libp2prs_core::transport::TransportError;
use libp2prs_core::upgrade::ProtocolName;

use crate::protocol_handler::IProtocolHandler;
use crate::ProtocolId;

/// [`Muxer`] is used by [`Swarm`] to select and handle protocols upgrading for inbound substream.
///
/// Multistream select protocol is used to proceed the protocol negotiation and selection.
///
/// IProtocolHandler is the tait object of ProtocolHandler.
///
pub(crate) struct Muxer {
    protocol_handlers: FnvHashMap<ProtocolId, IProtocolHandler>,
}

impl Clone for Muxer {
    fn clone(&self) -> Self {
        Muxer {
            protocol_handlers: self.protocol_handlers.clone(),
        }
    }
}

impl Default for Muxer {
    fn default() -> Self {
        Muxer::new()
    }
}

impl Muxer {
    /// Add `Muxer` on top of any `ProtoclHandler`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new() -> Self {
        Self {
            protocol_handlers: Default::default(),
        }
    }
}

impl Muxer {
    pub(crate) fn add_protocol_handler(&mut self, p: IProtocolHandler) {
        log::trace!(
            "adding protocol handler: {:?}",
            p.protocol_info().iter().map(|n| n.protocol_name_str()).collect::<Vec<_>>()
        );
        p.protocol_info().iter().for_each(|pid| {
            self.protocol_handlers.insert(pid, p.clone());
        });
    }

    pub(crate) fn supported_protocols(&self) -> impl IntoIterator<Item = ProtocolId> + '_ {
        self.protocol_handlers.keys().copied().map(|k| <&[u8]>::clone(&k))
    }

    pub(crate) async fn select_inbound(
        &mut self,
        socket: IReadWrite,
    ) -> Result<(IProtocolHandler, IReadWrite, ProtocolId), TransportError> {
        let protocols = self.supported_protocols();
        let negotiator = Negotiator::new_with_protocols(protocols);

        let (proto, socket) = negotiator.negotiate(socket).await?;
        let handler = self.protocol_handlers.get_mut(proto.as_ref()).unwrap().clone();

        log::info!("select_inbound {:?}", proto.protocol_name_str());

        Ok((handler as IProtocolHandler, socket, proto))
    }
}
