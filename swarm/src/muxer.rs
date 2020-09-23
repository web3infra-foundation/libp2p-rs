use fnv::FnvHashMap;
use libp2p_traits::{Read2, Write2};
use libp2p_core::multistream::Negotiator;
use libp2p_core::upgrade::ProtocolName;
use libp2p_core::transport::TransportError;
use crate::ProtocolId;
use crate::protocol_handler::BoxHandler;
use crate::substream::Substream;


/// Muxer that uses multistream-select to select and handle protocols.
///
pub struct Muxer<TRaw> {
    protocol_handlers: FnvHashMap<ProtocolId, BoxHandler<Substream<TRaw>>>,
}

impl<TRaw> Clone for Muxer<TRaw> {
    fn clone(&self) -> Self {
        Muxer {
            protocol_handlers: self.protocol_handlers.clone(),
        }
    }
}

impl<TRaw> Muxer<TRaw> {
    /// Add `Muxer` on top of any `ProtoclHandler`·
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new() -> Self {
        Self {
            protocol_handlers: Default::default()
        }
    }
}

impl<TRaw> Muxer<TRaw> {
    pub fn add_protocol_handler(&mut self, p: BoxHandler<Substream<TRaw>>) {
        log::trace!("adding protocol handler: {:?}", p.protocol_info().iter().map(|n|n.protocol_name_str()).collect::<Vec<_>>());
        p.protocol_info().iter().for_each(|pid| {
            self.protocol_handlers.insert(pid, p.clone());
        });
    }

    pub(crate) fn supported_protocols(&self) -> impl IntoIterator<Item=ProtocolId> + '_ {
        self.protocol_handlers.keys()
            .into_iter()
            .map(|k| k.clone())
    }

    pub(crate) async fn select_inbound(&mut self, socket: TRaw) -> Result<(BoxHandler<Substream<TRaw>>, TRaw, ProtocolId), TransportError>
        where
            TRaw: Read2 + Write2 + Send + Unpin,
    {
        let protocols = self.supported_protocols();
        let negotiator = Negotiator::new_with_protocols(protocols);

        let (proto, socket) = negotiator.negotiate(socket).await?;
        let handler = self.protocol_handlers.get_mut(proto.as_ref()).unwrap().clone();

        log::info!("select_inbound {:?}", proto.protocol_name_str());

        Ok((handler, socket, proto))
    }
}
