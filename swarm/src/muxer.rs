use crate::protocol_handler::IProtocolHandler;
use crate::ProtocolId;
use fnv::FnvHashMap;
use libp2p_core::multistream::Negotiator;
use libp2p_core::muxing::{IReadWrite, WrapIReadWrite};
use libp2p_core::transport::TransportError;
use libp2p_core::upgrade::ProtocolName;

/// Muxer that uses multistream-select to select and handle protocols.
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
        let wrap_socket = WrapIReadWrite::from(socket);

        let (proto, wrap_socket) = negotiator.negotiate(wrap_socket).await?;
        let handler = self.protocol_handlers.get_mut(proto.as_ref()).unwrap().clone();

        log::info!("select_inbound {:?}", proto.protocol_name_str());

        Ok((handler as IProtocolHandler, wrap_socket.into(), proto))
    }
}
