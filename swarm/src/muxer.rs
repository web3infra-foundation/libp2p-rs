use fnv::FnvHashMap;
use libp2p_core::multistream::Negotiator;
use libp2p_core::upgrade::ProtocolName;
use libp2p_traits::{Read2, Write2};
use crate::ProtocolId;
use crate::protocol_handler::BoxHandler;
use libp2p_core::transport::TransportError;


/// Muxer that uses multistream-select to select and handle protocols.
///
pub struct Muxer<TSubstream> {
    protocol_handlers: FnvHashMap<ProtocolId, BoxHandler<TSubstream>>,
}

impl<TSubstream> Clone for Muxer<TSubstream> {
    fn clone(&self) -> Self {
        Muxer {
            protocol_handlers: self.protocol_handlers.clone(),
        }
    }
}

impl<TSubstream> Muxer<TSubstream> {
    /// Add `Muxer` on top of any `ProtoclHandler`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new() -> Self {
        Self {
            protocol_handlers: Default::default()
        }
    }
}

impl<TSubstream> Muxer<TSubstream> {
    pub fn add_protocol_handler(&mut self, p: BoxHandler<TSubstream>) {
        log::trace!("adding protocol handler: {:?}", p.protocol_info().iter().map(|n|n.protocol_name_str()).collect::<Vec<_>>());
        p.protocol_info().iter().for_each(|pid| {
            self.protocol_handlers.insert(pid, p.clone());
        });
    }

    pub(crate) async fn select_inbound(&mut self, socket: TSubstream) -> Result<(BoxHandler<TSubstream>, TSubstream, ProtocolId), TransportError>
        where
            TSubstream: Read2 + Write2 + Send + Unpin,
    {
        //let protocols = self.protocol_handlers.keys();
        let negotiator = Negotiator::new_with_protocols(vec![b"/dummy/2.0.0"]);

        let (proto, socket) = negotiator.negotiate(socket).await?;
        //let proto = b"www";

        let handler = self.protocol_handlers.get_mut(proto.as_ref()).unwrap().clone();

        log::info!("select_inbound {:?}", proto.protocol_name_str());

        Ok((handler, socket, proto))
    }
}
