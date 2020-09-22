use fnv::FnvHashMap;
use libp2p_core::multistream::Negotiator;
use libp2p_core::upgrade::ProtocolName;
use libp2p_traits::{Read2, Write2};
use crate::{ProtocolId, SwarmError};
use crate::protocol_handler::BoxHandler;



/// Muxer that uses multistream-select to select and handle protocols.
///
///
pub struct Muxer<TSubstream> {
    negotiator: Negotiator<ProtocolId>,
    protocol_handlers: FnvHashMap<ProtocolId, BoxHandler<TSubstream>>,
}

impl<TSubstream> Muxer<TSubstream> {
    /// Add `Muxer` on top of any `ProtoclHandler`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new() -> Self {
        Self {
            negotiator: Default::default(),
            protocol_handlers: Default::default()
        }
    }
}

impl<TSubstream> Muxer<TSubstream> {
    pub fn add_protocol_handler(&mut self, p: BoxHandler<TSubstream>) {
        log::trace!("adding protocol handler: {:?}", p.protocol_info().iter().map(|n|n.protocol_name_str()).collect::<Vec<_>>());
        p.protocol_info().iter().for_each(|pid| {
            self.protocol_handlers.insert(pid, p.clone());
            let _ = self.negotiator.add_protocol(pid);
        });
    }

    pub(crate) async fn select_inbound(&mut self, socket: TSubstream) -> Result<(BoxHandler<TSubstream>, TSubstream, ProtocolId), SwarmError>
        where
            TSubstream: Read2 + Write2 + Send + Unpin,
    {
        let (proto, socket) = self.negotiator.negotiate(socket).await.map_err(|e|SwarmError::Transport(e.into()))?;
        let handler = self.protocol_handlers.get_mut(proto.clone()).unwrap().clone();

        log::info!("select_inbound {:?}", proto.protocol_name_str());

        Ok((handler, socket, proto))
    }
}
