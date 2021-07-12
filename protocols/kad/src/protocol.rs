// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! The Kademlia connection protocol upgrade and associated message types.
//!
//! The connection protocol upgrade is provided by [`KadProtocolHandler`], with the
//! request and response types [`KadRequestMsg`] and [`KadResponseMsg`], respectively.
//! The upgrade's output is a `Sink + Stream` of messages. The `Stream` component is used
//! to poll the underlying transport for incoming messages, and the `Sink` component
//! is used to send messages to remote peers.

use futures::channel::oneshot;
use prost::Message;
use std::error::Error;
use std::io;
use std::{convert::TryFrom, time::Duration};

use async_trait::async_trait;

use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId, ProtocolId, ReadEx, WriteEx};
use libp2prs_swarm::connection::Connection;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler};
use libp2prs_swarm::substream::{Substream, SubstreamView};
use libp2prs_swarm::Control as SwarmControl;

use crate::kad::KadPoster;
use crate::record::{self, Record};
use crate::{dht_proto as proto, KadError, ProviderRecord};

/// The protocol name used for negotiating with multistream-select.
pub const DEFAULT_PROTO_NAME: &[u8] = b"/ipfs/kad/1.0.0";

/// The default maximum size for a varint length-delimited packet.
pub const DEFAULT_MAX_PACKET_SIZE: usize = 16 * 1024;

/// The number of times we will try to reuse a stream to a given peer.
pub const DEFAULT_MAX_REUSE_TRIES: usize = 3;

/// Status of our connection to a node reported by the Kademlia protocol.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum KadConnectionType {
    /// Sender hasn't tried to connect to peer.
    NotConnected = 0,
    /// Sender is currently connected to peer.
    Connected = 1,
    /// Sender was recently connected to peer.
    CanConnect = 2,
    /// Sender tried to connect to peer but failed.
    CannotConnect = 3,
}

impl From<proto::message::ConnectionType> for KadConnectionType {
    fn from(raw: proto::message::ConnectionType) -> KadConnectionType {
        use proto::message::ConnectionType::*;
        match raw {
            NotConnected => KadConnectionType::NotConnected,
            Connected => KadConnectionType::Connected,
            CanConnect => KadConnectionType::CanConnect,
            CannotConnect => KadConnectionType::CannotConnect,
        }
    }
}

impl Into<proto::message::ConnectionType> for KadConnectionType {
    fn into(self) -> proto::message::ConnectionType {
        use proto::message::ConnectionType::*;
        match self {
            KadConnectionType::NotConnected => NotConnected,
            KadConnectionType::Connected => Connected,
            KadConnectionType::CanConnect => CanConnect,
            KadConnectionType::CannotConnect => CannotConnect,
        }
    }
}

/// Information about a peer, as known by the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadPeer {
    /// Identifier of the peer.
    pub node_id: PeerId,
    /// The multiaddresses that the sender think can be used in order to reach the peer.
    pub multiaddrs: Vec<Multiaddr>,
    /// How the sender is connected to that remote.
    pub connection_ty: KadConnectionType,
}

// Builds a `KadPeer` from a corresponding protobuf message.
impl TryFrom<proto::message::Peer> for KadPeer {
    type Error = io::Error;

    fn try_from(peer: proto::message::Peer) -> Result<KadPeer, Self::Error> {
        // TODO: this is in fact a CID; not sure if this should be handled in `from_bytes` or
        //       as a special case here
        let node_id = PeerId::from_bytes(&peer.id).map_err(|_| invalid_data("invalid peer id"))?;

        let mut addrs = Vec::with_capacity(peer.addrs.len());
        for addr in peer.addrs.into_iter() {
            let as_ma = Multiaddr::try_from(addr).map_err(invalid_data)?;
            addrs.push(as_ma);
        }
        debug_assert_eq!(addrs.len(), addrs.capacity());

        let connection_ty = proto::message::ConnectionType::from_i32(peer.connection)
            .ok_or_else(|| invalid_data("unknown connection type"))?
            .into();

        Ok(KadPeer {
            node_id,
            multiaddrs: addrs,
            connection_ty,
        })
    }
}

impl Into<proto::message::Peer> for KadPeer {
    fn into(self) -> proto::message::Peer {
        proto::message::Peer {
            id: self.node_id.to_bytes(),
            addrs: self.multiaddrs.into_iter().map(|a| a.to_vec()).collect(),
            connection: {
                let ct: proto::message::ConnectionType = self.connection_ty.into();
                ct as i32
            },
        }
    }
}

impl Into<PeerId> for KadPeer {
    fn into(self) -> PeerId {
        self.node_id
    }
}

/// Configuration for a Kademlia protocol handler.
#[derive(Debug, Clone)]
pub struct KademliaProtocolConfig {
    /// The Kademlia protocol name, e.g., b"/ipfs/kad/1.0.0".
    protocol_name: ProtocolId,
    /// Maximum allowed size of a packet.
    max_packet_size: usize,
    /// Maximum allowed reuse count of a substream, used by Messenger cache.
    max_reuse_count: usize,
    /// Timeout of new substream.
    new_stream_timeout:  Option<Duration>,
    /// Timeout of substream read and write.
    read_write_timeout: Option<Duration>,
}

impl KademliaProtocolConfig {
    /// Returns the configured protocol name.
    pub fn protocol_name(&self) -> &ProtocolId {
        &self.protocol_name
    }

    /// Modifies the protocol name used on the wire. Can be used to create incompatibilities
    /// between networks on purpose.
    pub fn set_protocol_name(&mut self, name: ProtocolId) {
        self.protocol_name = name;
    }

    /// Modifies the maximum allowed size of a single Kademlia packet.
    pub fn set_max_packet_size(&mut self, size: usize) {
        self.max_packet_size = size;
    }

    /// Set timeout of new substream.
    pub fn set_new_stream_timeout(&mut self, timeout: Duration) {
        self.new_stream_timeout = Some(timeout);
    }

    /// Set timeout of substream read and write.
    pub fn set_read_write_timeout(&mut self, timeout: Duration) {
        self.read_write_timeout = Some(timeout);
    }
}

impl Default for KademliaProtocolConfig {
    fn default() -> Self {
        KademliaProtocolConfig {
            protocol_name: ProtocolId::new(DEFAULT_PROTO_NAME, 0),
            max_packet_size: DEFAULT_MAX_PACKET_SIZE,
            max_reuse_count: DEFAULT_MAX_REUSE_TRIES,
            new_stream_timeout: None,
            read_write_timeout: None
        }
    }
}

/// The Protocol Handler of Kademlia DHT.
#[derive(Debug, Clone)]
pub struct KadProtocolHandler {
    /// The configuration of the protocol handler.
    config: KademliaProtocolConfig,
    /// If false, we deny incoming requests.
    allow_listening: bool,
    /// Time after which we close an idle connection.
    idle_timeout: Duration,
    /// Used to post ProtocolEvent to Kad main loop.
    poster: KadPoster,
}

impl KadProtocolHandler {
    /// Make a new KadProtocolHandler.
    pub(crate) fn new(config: KademliaProtocolConfig, poster: KadPoster) -> Self {
        KadProtocolHandler {
            config,
            allow_listening: false,
            idle_timeout: Duration::from_secs(10),
            poster,
        }
    }

    /// Returns the configured protocol name.
    pub fn protocol_name(&self) -> &ProtocolId {
        &self.config.protocol_name
    }

    /// Modifies the maximum allowed size of a single Kademlia packet.
    pub fn set_max_packet_size(&mut self, size: usize) {
        self.config.max_packet_size = size;
    }
}

impl UpgradeInfo for KadProtocolHandler {
    type Info = ProtocolId;

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![self.config.protocol_name.to_owned()]
    }
}

impl Notifiee for KadProtocolHandler {
    fn connected(&mut self, conn: &mut Connection) {
        let peer_id = conn.remote_peer();
        let _ = self.poster.unbounded_post(ProtocolEvent::PeerConnected(peer_id));
    }
    fn disconnected(&mut self, conn: &mut Connection) {
        let peer_id = conn.remote_peer();
        let _ = self.poster.unbounded_post(ProtocolEvent::PeerDisconnected(peer_id));
    }
    fn identified(&mut self, peer_id: PeerId) {
        let _ = self.poster.unbounded_post(ProtocolEvent::PeerIdentified(peer_id));
    }
    fn address_changed(&mut self, addrs: Vec<Multiaddr>) {
        let _ = self.poster.unbounded_post(ProtocolEvent::AddressChanged(addrs));
    }
}

#[async_trait]
impl ProtocolHandler for KadProtocolHandler {
    async fn handle(&mut self, mut stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
        let source = stream.remote_peer();
        log::trace!("Kad Handler opened for remote {:?}", source);
        loop {
            let packet = stream.read_one(self.config.max_packet_size).await?;
            let request = proto::Message::decode(&packet[..]).map_err(|_| KadError::Decode)?;
            log::trace!("Kad handler recv : {:?}", request);

            let request = proto_to_req_msg(request)?;

            // For AddProvider request, KadResponse is not needed
            let (tx, rx) = oneshot::channel();
            let evt = ProtocolEvent::KadRequest {
                request,
                source,
                reply: tx,
            };
            self.poster.post(evt).await?;
            let response = rx.await??;

            if let Some(response) = response {
                // handle response messages
                let proto_struct = resp_msg_to_proto(response);
                let mut buf = Vec::with_capacity(proto_struct.encoded_len());
                proto_struct.encode(&mut buf).expect("Vec<u8> provides capacity as needed");

                let _ = stream.write_one(&buf).await?;
            }
        }
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

/// Kademlia messenger.
///
/// The messenger actually sends a Kad request message and waits for the correct response
/// message.
pub(crate) struct KadMessenger {
    /// The substream for I/O.
    pub(crate) stream: Substream,
    /// The configuration of Kad protocol.
    pub(crate) config: KademliaProtocolConfig,
    /// The PeerId which this messenger is connected to.
    peer: PeerId,
    /// The reuse count, >=3 will be recycled.
    reuse: usize,
}

/// View for Debugging purpose.
#[derive(Debug)]
pub struct KadMessengerView {
    pub peer: PeerId,
    pub stream: SubstreamView,
    pub reuse: usize,
}

impl KadMessenger {
    pub(crate) async fn build(mut swarm: SwarmControl, peer: PeerId, config: KademliaProtocolConfig) -> Result<Self, KadError> {
        // open a new stream, without routing, so that Swarm wouldn't do routing for us
        let stream = swarm.new_stream_no_routing_with_timeout(peer, vec![config.protocol_name().to_owned()], config.new_stream_timeout).await?.with_timeout(config.read_write_timeout);
        Ok(Self {
            stream,
            config,
            peer,
            reuse: 0,
        })
    }

    pub(crate) fn to_view(&self) -> KadMessengerView {
        KadMessengerView {
            peer: self.peer,
            stream: self.stream.to_view(),
            reuse: self.reuse,
        }
    }

    pub(crate) fn get_peer_id(&self) -> &PeerId {
        &self.peer
    }

    // update reuse count, true means yes please reuse, otherwise, messenger is recycled
    pub(crate) fn reuse(&mut self) -> bool {
        self.reuse += 1;
        self.reuse < self.config.max_reuse_count
    }

    // send a message to peer.
    async fn send_message(&mut self, request: KadRequestMsg) -> Result<(), KadError> {
        let proto_struct = req_msg_to_proto(request);
        let mut buf = Vec::with_capacity(proto_struct.encoded_len());
        proto_struct.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        self.stream.write_one(&buf).await?;

        Ok(())
    }

    // send a request message to peer , then wait for the response.
    async fn send_request(&mut self, request: KadRequestMsg) -> Result<KadResponseMsg, KadError> {
        let proto_struct = req_msg_to_proto(request);
        let mut buf = Vec::with_capacity(proto_struct.encoded_len());
        proto_struct.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        self.stream.write_one(&buf).await?;

        let packet = self.stream.read_one(self.config.max_packet_size).await?;
        let response = proto::Message::decode(&packet[..]).map_err(|_| KadError::Decode)?;
        log::trace!("Kad handler recv : {:?}", response);

        let response = proto_to_resp_msg(response)?;

        Ok(response)
    }

    pub(crate) async fn send_find_node(&mut self, key: record::Key) -> Result<Vec<KadPeer>, KadError> {
        let req = KadRequestMsg::FindNode { key };
        let rsp = self.send_request(req).await?;
        match rsp {
            KadResponseMsg::FindNode { closer_peers } => Ok(closer_peers),
            _ => Err(KadError::UnexpectedMessage("wrong message type received when FindNode")),
        }
    }

    pub(crate) async fn send_get_providers(&mut self, key: record::Key) -> Result<(Vec<KadPeer>, Vec<KadPeer>), KadError> {
        let req = KadRequestMsg::GetProviders { key };
        let rsp = self.send_request(req).await?;
        match rsp {
            KadResponseMsg::GetProviders {
                closer_peers,
                provider_peers,
            } => Ok((closer_peers, provider_peers)),
            _ => Err(KadError::UnexpectedMessage("wrong message type received when GetProviders")),
        }
    }

    pub(crate) async fn send_add_provider(
        &mut self,
        provider_record: ProviderRecord,
        addresses: Vec<Multiaddr>,
    ) -> Result<(), KadError> {
        let provider = KadPeer {
            node_id: provider_record.provider,
            multiaddrs: addresses,
            connection_ty: KadConnectionType::Connected,
        };

        let req = KadRequestMsg::AddProvider {
            key: provider_record.key,
            provider,
        };
        self.send_message(req).await?;
        Ok(())
    }

    pub(crate) async fn send_get_value(&mut self, key: record::Key) -> Result<(Vec<KadPeer>, Option<Record>), KadError> {
        let req = KadRequestMsg::GetValue { key };
        let rsp = self.send_request(req).await?;
        match rsp {
            KadResponseMsg::GetValue { record, closer_peers } => Ok((closer_peers, record)),
            _ => Err(KadError::UnexpectedMessage("wrong message type received when GetValue")),
        }
    }

    pub(crate) async fn send_put_value(&mut self, record: Record) -> Result<(record::Key, Vec<u8>), KadError> {
        let req = KadRequestMsg::PutValue { record };
        let rsp = self.send_request(req).await?;
        match rsp {
            KadResponseMsg::PutValue { key, value } => Ok((key, value)),
            _ => Err(KadError::UnexpectedMessage("wrong message type received when PutValue")),
        }
    }
}

/// Request that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadRequestMsg {
    /// Ping request.
    Ping,

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNode {
        /// The key for which to locate the closest nodes.
        key: record::Key,
    },

    /// Same as `FindNode`, but should also return the entries of the local providers list for
    /// this key.
    GetProviders {
        /// Identifier being searched.
        key: record::Key,
    },

    /// Indicates that this list of providers is known for this key.
    AddProvider {
        /// Key for which we should add providers.
        key: record::Key,
        /// Known provider for this key.
        provider: KadPeer,
    },

    /// Request to get a value from the dht records.
    GetValue {
        /// The key we are searching for.
        key: record::Key,
    },

    /// Request to put a value into the dht records.
    PutValue { record: Record },
}

/// Response that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadResponseMsg {
    /// Ping response.
    Pong,

    /// Response to a `FindNode`.
    FindNode {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
    },

    /// Response to a `GetProviders`.
    GetProviders {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
    },

    /// Response to a `GetValue`.
    GetValue {
        /// Result that might have been found
        record: Option<Record>,
        /// Nodes closest to the key
        closer_peers: Vec<KadPeer>,
    },

    /// Response to a `PutValue`.
    PutValue {
        /// The key of the record.
        key: record::Key,
        /// Value of the record.
        value: Vec<u8>,
    },
}

/// Converts a `KadRequestMsg` into the corresponding protobuf message for sending.
fn req_msg_to_proto(kad_msg: KadRequestMsg) -> proto::Message {
    match kad_msg {
        KadRequestMsg::Ping => proto::Message {
            r#type: proto::message::MessageType::Ping as i32,
            ..proto::Message::default()
        },
        KadRequestMsg::FindNode { key } => proto::Message {
            r#type: proto::message::MessageType::FindNode as i32,
            key: key.to_vec(),
            cluster_level_raw: 10,
            ..proto::Message::default()
        },
        KadRequestMsg::GetProviders { key } => proto::Message {
            r#type: proto::message::MessageType::GetProviders as i32,
            key: key.to_vec(),
            cluster_level_raw: 10,
            ..proto::Message::default()
        },
        KadRequestMsg::AddProvider { key, provider } => proto::Message {
            r#type: proto::message::MessageType::AddProvider as i32,
            cluster_level_raw: 10,
            key: key.to_vec(),
            provider_peers: vec![provider.into()],
            ..proto::Message::default()
        },
        KadRequestMsg::GetValue { key } => proto::Message {
            r#type: proto::message::MessageType::GetValue as i32,
            cluster_level_raw: 10,
            key: key.to_vec(),
            ..proto::Message::default()
        },
        KadRequestMsg::PutValue { record } => proto::Message {
            r#type: proto::message::MessageType::PutValue as i32,
            record: Some(record_to_proto(record)),
            ..proto::Message::default()
        },
    }
}

/// Converts a `KadResponseMsg` into the corresponding protobuf message for sending.
fn resp_msg_to_proto(kad_msg: KadResponseMsg) -> proto::Message {
    match kad_msg {
        KadResponseMsg::Pong => proto::Message {
            r#type: proto::message::MessageType::Ping as i32,
            ..proto::Message::default()
        },
        KadResponseMsg::FindNode { closer_peers } => proto::Message {
            r#type: proto::message::MessageType::FindNode as i32,
            cluster_level_raw: 9,
            closer_peers: closer_peers.into_iter().map(KadPeer::into).collect(),
            ..proto::Message::default()
        },
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => proto::Message {
            r#type: proto::message::MessageType::GetProviders as i32,
            cluster_level_raw: 9,
            closer_peers: closer_peers.into_iter().map(KadPeer::into).collect(),
            provider_peers: provider_peers.into_iter().map(KadPeer::into).collect(),
            ..proto::Message::default()
        },
        KadResponseMsg::GetValue { record, closer_peers } => proto::Message {
            r#type: proto::message::MessageType::GetValue as i32,
            cluster_level_raw: 9,
            closer_peers: closer_peers.into_iter().map(KadPeer::into).collect(),
            record: record.map(record_to_proto),
            ..proto::Message::default()
        },
        KadResponseMsg::PutValue { key, value } => proto::Message {
            r#type: proto::message::MessageType::PutValue as i32,
            key: key.to_vec(),
            record: Some(proto::Record {
                key: key.to_vec(),
                value,
                ..proto::Record::default()
            }),
            ..proto::Message::default()
        },
    }
}

/// Converts a received protobuf message into a corresponding `KadRequestMsg`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia request message.
fn proto_to_req_msg(message: proto::Message) -> Result<KadRequestMsg, io::Error> {
    let msg_type = proto::message::MessageType::from_i32(message.r#type)
        .ok_or_else(|| invalid_data(format!("unknown message type: {}", message.r#type)))?;

    match msg_type {
        proto::message::MessageType::Ping => Ok(KadRequestMsg::Ping),
        proto::message::MessageType::PutValue => {
            let record = record_from_proto(message.record.unwrap_or_default())?;
            Ok(KadRequestMsg::PutValue { record })
        }
        proto::message::MessageType::GetValue => Ok(KadRequestMsg::GetValue {
            key: record::Key::from(message.key),
        }),
        proto::message::MessageType::FindNode => Ok(KadRequestMsg::FindNode {
            key: record::Key::from(message.key),
        }),
        proto::message::MessageType::GetProviders => Ok(KadRequestMsg::GetProviders {
            key: record::Key::from(message.key),
        }),
        proto::message::MessageType::AddProvider => {
            // TODO: for now we don't parse the peer properly, so it is possible that we get
            //       parsing errors for peers even when they are valid; we ignore these
            //       errors for now, but ultimately we should just error altogether
            let provider = message.provider_peers.into_iter().find_map(|peer| KadPeer::try_from(peer).ok());

            if let Some(provider) = provider {
                let key = record::Key::from(message.key);
                Ok(KadRequestMsg::AddProvider { key, provider })
            } else {
                Err(invalid_data("AddProvider message with no valid peer."))
            }
        }
    }
}

/// Converts a received protobuf message into a corresponding `KadResponseMessage`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia response message.
fn proto_to_resp_msg(message: proto::Message) -> Result<KadResponseMsg, io::Error> {
    let msg_type = proto::message::MessageType::from_i32(message.r#type)
        .ok_or_else(|| invalid_data(format!("unknown message type: {}", message.r#type)))?;

    match msg_type {
        proto::message::MessageType::Ping => Ok(KadResponseMsg::Pong),
        proto::message::MessageType::GetValue => {
            let record = if let Some(r) = message.record {
                Some(record_from_proto(r)?)
            } else {
                None
            };

            let closer_peers = message
                .closer_peers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::GetValue { record, closer_peers })
        }

        proto::message::MessageType::FindNode => {
            let closer_peers = message
                .closer_peers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::FindNode { closer_peers })
        }

        proto::message::MessageType::GetProviders => {
            let closer_peers = message
                .closer_peers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            let provider_peers = message
                .provider_peers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::GetProviders {
                closer_peers,
                provider_peers,
            })
        }

        proto::message::MessageType::PutValue => {
            let key = record::Key::from(message.key);
            let rec = message
                .record
                .ok_or_else(|| invalid_data("received PutValue message with no record"))?;

            Ok(KadResponseMsg::PutValue { key, value: rec.value })
        }

        proto::message::MessageType::AddProvider => Err(invalid_data("received an unexpected AddProvider message")),
    }
}

fn record_from_proto(record: proto::Record) -> Result<Record, io::Error> {
    let key = record::Key::from(record.key);
    let value = record.value;

    let publisher = if !record.publisher.is_empty() {
        PeerId::from_bytes(&record.publisher)
            .map(Some)
            .map_err(|_| invalid_data("Invalid publisher peer ID."))?
    } else {
        None
    };

    let record = Record::new(key, value, false, publisher);

    Ok(record)
}

fn record_to_proto(record: Record) -> proto::Record {
    proto::Record {
        key: record.key.to_vec(),
        value: record.value,
        publisher: record.publisher.map(|id| id.to_bytes()).unwrap_or_default(),
        time_received: String::new(),
    }
}

/// Creates an `io::Error` with `io::ErrorKind::InvalidData`.
fn invalid_data<E>(e: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {

    /*// TODO: restore
    use self::libp2p_tcp::TcpConfig;
    use self::tokio::runtime::current_thread::Runtime;
    use futures::{Future, Sink, Stream};
    use libp2p_core::{PeerId, PublicKey, Transport};
    use multihash::{encode, Hash};
    use protocol::{KadConnectionType, KadPeer, KadProtocolHandler};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send a message between the two, and check that they were
        // successfully received.

        test_one(KadMsg::Ping);
        test_one(KadMsg::FindNodeReq {
            key: PeerId::random(),
        });
        test_one(KadMsg::FindNodeRes {
            closer_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            }],
        });
        test_one(KadMsg::GetProvidersReq {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
        });
        test_one(KadMsg::GetProvidersRes {
            closer_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            }],
            provider_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/200.201.202.203/tcp/1999".parse().unwrap()],
                connection_ty: KadConnectionType::NotConnected,
            }],
        });
        test_one(KadMsg::AddProvider {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
            provider_peer: KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/9.1.2.3/udp/23".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            },
        });
        // TODO: all messages

        fn test_one(msg_server: KadMsg) {
            let msg_client = msg_server.clone();
            let (tx, rx) = mpsc::channel();

            let bg_thread = thread::spawn(move || {
                let transport = TcpConfig::new().with_upgrade(KadProtocolHandler);

                let (listener, addr) = transport
                    .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                    .unwrap();
                tx.send(addr).unwrap();

                let future = listener
                    .into_future()
                    .map_err(|(err, _)| err)
                    .and_then(|(client, _)| client.unwrap().0)
                    .and_then(|proto| proto.into_future().map_err(|(err, _)| err).map(|(v, _)| v))
                    .map(|recv_msg| {
                        assert_eq!(recv_msg.unwrap(), msg_server);
                        ()
                    });
                let mut rt = Runtime::new().unwrap();
                let _ = rt.block_on(future).unwrap();
            });

            let transport = TcpConfig::new().with_upgrade(KadProtocolHandler);

            let future = transport
                .dial(rx.recv().unwrap())
                .unwrap()
                .and_then(|proto| proto.send(msg_client))
                .map(|_| ());
            let mut rt = Runtime::new().unwrap();
            let _ = rt.block_on(future).unwrap();
            bg_thread.join().unwrap();
        }
    }*/
}

#[derive(Debug)]
pub enum RefreshStage {
    Start(Option<oneshot::Sender<Result<(), KadError>>>),
    SelfQueryDone(Option<oneshot::Sender<Result<(), KadError>>>),
    Completed,
}

/// Event produced by the Kademlia handler.
#[derive(Debug)]
pub(crate) enum ProtocolEvent {
    /// Refresh state.
    Refresh(RefreshStage),

    /// A new connection from peer_id is opened.
    ///
    /// This notification comes from Protocol Notifiee trait.
    PeerConnected(PeerId),
    /// A connection from peer_id is closed.
    ///
    /// This notification comes from Protocol Notifiee trait.
    PeerDisconnected(PeerId),
    /// A remote peer has been identified.
    ///
    /// It means PeerStore must have the Multiaddr and Protocols
    /// information of the peer.
    ///
    /// This notification comes from Protocol Notifiee trait.
    PeerIdentified(PeerId),
    /// The local address change is detected.
    ///
    /// In fact the local address might change for many reasons. f.g.,
    /// interface up/down. When our address changes, we should proactively
    /// tell our closest peers about it so we become discoverable quickly.
    ///
    /// This notification comes from Protocol Notifiee trait.
    AddressChanged(Vec<Multiaddr>),

    /// A new peer found when trying to query a 'Key' or receiving a
    /// query from peer, which obviously implies we are talking to an
    /// valid and live Kad Peer, since we have done protocol negotiating
    /// with it.
    ///
    /// This notification comes from either a query or a forced activity.
    /// The 'bool' parameter 'true' means if it comes from a query.
    KadPeerFound(PeerId, bool),
    /// A Kad peer stopped when it is being connected/queried.
    ///
    /// Typically, this comes from either a query activity or an event
    /// notification from event bus(TBD).
    KadPeerStopped(PeerId),

    /// Timer event for Provider & Record GC.
    GCTimer,

    /// Timer event for Refresh.
    RefreshTimer,

    /// Kad request message from remote peer.
    ///
    KadRequest {
        /// Request message, decoded from ProtoBuf
        request: KadRequestMsg,
        /// Source of the message, which is the Peer Id of the remote.
        source: PeerId,
        /// Reply oneshot channel.
        ///
        /// Note that AddProvider doesn't require response. This is why
        /// KadResponseMsg is wrapped by Option<T>
        reply: oneshot::Sender<Result<Option<KadResponseMsg>, KadError>>,
    },
}
