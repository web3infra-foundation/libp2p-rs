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

use prost::Message;
use std::{error::Error, fmt, io};

use crate::floodsub::PeerEvent;
use crate::{rpc_proto, Topic, FLOOD_SUB_ID};
use async_std::task;
use async_trait::async_trait;
use futures::{channel::mpsc, SinkExt};
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{PeerId, ProtocolId};
use libp2prs_swarm::protocol_handler::Notifiee;
use libp2prs_swarm::{
    connection::Connection,
    protocol_handler::{IProtocolHandler, ProtocolHandler},
    substream::Substream,
};
use libp2prs_traits::{ReadEx, WriteEx};

#[derive(Clone)]
pub struct Handler {
    incoming_tx: mpsc::UnboundedSender<RPC>,
    new_peer: mpsc::UnboundedSender<PeerEvent>,
}

impl Handler {
    pub(crate) fn new(incoming_tx: mpsc::UnboundedSender<RPC>, new_peer: mpsc::UnboundedSender<PeerEvent>) -> Self {
        Handler { incoming_tx, new_peer }
    }
}

impl UpgradeInfo for Handler {
    type Info = ProtocolId;

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![FLOOD_SUB_ID.into()]
    }
}

impl Notifiee for Handler {
    fn connected(&mut self, conn: &mut Connection) {
        let peer_id = conn.remote_peer();
        let mut new_peers = self.new_peer.clone();
        task::spawn(async move {
            let _ = new_peers.send(PeerEvent::NewPeer(peer_id)).await;
        });
    }
}

#[async_trait]
impl ProtocolHandler for Handler {
    async fn handle(&mut self, mut stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
        log::trace!("Handle stream from {}", stream.remote_peer());
        loop {
            let packet = match stream.read_one(2048).await {
                Ok(p) => p,
                Err(e) => {
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        stream.close2().await?;
                    }
                    return Err(Box::new(e));
                }
            };
            let rpc = rpc_proto::Rpc::decode(&packet[..])?;
            log::trace!("recv rpc msg: {:?}", rpc);

            let mut messages = Vec::with_capacity(rpc.publish.len());
            for publish in rpc.publish.into_iter() {
                messages.push(FloodsubMessage {
                    source: PeerId::from_bytes(publish.from.unwrap_or_default()).map_err(|_| FloodsubDecodeError::InvalidPeerId)?,
                    data: publish.data.unwrap_or_default(),
                    sequence_number: publish.seqno.unwrap_or_default(),
                    topics: publish.topic_ids.into_iter().map(Topic::new).collect(),
                });
            }

            let rpc = RPC {
                rpc: FloodsubRpc {
                    messages,
                    subscriptions: rpc
                        .subscriptions
                        .into_iter()
                        .map(|sub| FloodsubSubscription {
                            action: if Some(true) == sub.subscribe {
                                FloodsubSubscriptionAction::Subscribe
                            } else {
                                FloodsubSubscriptionAction::Unsubscribe
                            },
                            topic: Topic::new(sub.topic_id.unwrap_or_default()),
                        })
                        .collect(),
                },
                from: stream.remote_peer(),
            };

            self.incoming_tx.send(rpc).await.map_err(|_| FloodsubDecodeError::ProtocolExit)?;
        }
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

/// Reach attempt interrupt errors.
#[derive(Debug)]
pub enum FloodsubDecodeError {
    /// Error when reading the packet from the socket.
    ReadError(io::Error),
    /// Error when decoding the raw buffer into a protobuf.
    ProtobufError(prost::DecodeError),
    /// Error when parsing the `PeerId` in the message.
    InvalidPeerId,
    /// Protocol message process mainloop exit
    ProtocolExit,
}

impl From<prost::DecodeError> for FloodsubDecodeError {
    fn from(err: prost::DecodeError) -> Self {
        FloodsubDecodeError::ProtobufError(err)
    }
}

impl fmt::Display for FloodsubDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            FloodsubDecodeError::ReadError(ref err) => write!(f, "Error while reading from socket: {}", err),
            FloodsubDecodeError::ProtobufError(ref err) => write!(f, "Error while decoding protobuf: {}", err),
            FloodsubDecodeError::InvalidPeerId => write!(f, "Error while decoding PeerId from message"),
            FloodsubDecodeError::ProtocolExit => write!(f, "Error while send message to message process mainloop"),
        }
    }
}

impl Error for FloodsubDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            FloodsubDecodeError::ReadError(ref err) => Some(err),
            FloodsubDecodeError::ProtobufError(ref err) => Some(err),
            FloodsubDecodeError::InvalidPeerId => None,
            FloodsubDecodeError::ProtocolExit => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RPC {
    pub rpc: FloodsubRpc,
    // unexported on purpose, not sending this over the wire
    pub from: PeerId,
}

/// An RPC received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<FloodsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<FloodsubSubscription>,
}

impl FloodsubRpc {
    /// Turns this `FloodsubRpc` into a message that can be sent to a substream.
    pub fn into_bytes(self) -> Vec<u8> {
        let rpc = rpc_proto::Rpc {
            publish: self
                .messages
                .into_iter()
                .map(|msg| rpc_proto::Message {
                    from: Some(msg.source.into_bytes()),
                    data: Some(msg.data),
                    seqno: Some(msg.sequence_number),
                    topic_ids: msg.topics.into_iter().map(|topic| topic.into()).collect(),
                })
                .collect(),

            subscriptions: self
                .subscriptions
                .into_iter()
                .map(|topic| rpc_proto::rpc::SubOpts {
                    subscribe: Some(topic.action == FloodsubSubscriptionAction::Subscribe),
                    topic_id: Some(topic.topic.into()),
                })
                .collect(),
        };

        let mut buf = Vec::with_capacity(rpc.encoded_len());
        rpc.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        buf
    }
}

/// A message received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubMessage {
    /// Id of the peer that published this message.
    pub source: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// An incrementing sequence number.
    pub sequence_number: Vec<u8>,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<Topic>,
}

/// A subscription received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubSubscription {
    /// Action to perform.
    pub action: FloodsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic: Topic,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloodsubSubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}
