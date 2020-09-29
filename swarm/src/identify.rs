//! Implementation of the [Identify] protocol.
//!
//! This implementation of the protocol periodically exchanges
//! [`IdentifyInfo`] messages between the peers on an established connection.
//!
//! At least one identification request is sent on a newly established
//! connection, beyond which the behaviour does not keep connections alive.
//!
//! # Usage
//!
//! The [`Identify`] struct implements a `NetworkBehaviour` that negotiates
//! and executes the protocol on every established connection, emitting
//! [`IdentifyEvent`]s.
//!
//! [Identify]: https://github.com/libp2p/specs/tree/master/identify
//! [`Identify`]: self::Identify
//! [`IdentifyEvent`]: self::IdentifyEvent
//! [`IdentifyInfo`]: self::IdentifyEvent

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::SinkExt;
use prost::Message;
use std::convert::TryFrom;
use std::io;

use libp2p_core::muxing::StreamInfo;
use libp2p_core::transport::TransportError;
use libp2p_core::upgrade::UpgradeInfo;
use libp2p_core::{Multiaddr, PublicKey};
use libp2p_traits::{ReadEx, WriteEx};

use crate::protocol_handler::{IProtocolHandler, ProtocolHandler};
use crate::substream::Substream;
use crate::{SwarmError, SwarmEvent};

mod structs_proto {
    include!(concat!(env!("OUT_DIR"), "/structs.rs"));
}

pub const IDENTIFY_PROTOCOL: &[u8] = b"/ipfs/id/1.0.0";
pub const IDENTIFY_PUSH_PROTOCOL: &[u8] = b"/ipfs/id/push/1.0.0";

/// The configuration for identify.
#[derive(Clone, Debug, Default)]
pub struct IdentifyConfig {
    /// Starts the Push service.
    pub(crate) push: bool,
}

impl IdentifyConfig {
    pub fn new(push: bool) -> Self {
        Self {
            push,
        }
    }
}

/// Information of a peer sent in `Identify` protocol responses.
#[derive(Debug, Clone)]
pub struct IdentifyInfo {
    /// The public key underlying the peer's `PeerId`.
    pub public_key: PublicKey,
    /// Version of the protocol family used by the peer, e.g. `ipfs/1.0.0`
    /// or `p2p/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// The addresses that the peer is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// The list of protocols supported by the peer, e.g. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    /// Information about the remote.
    pub info: IdentifyInfo,
    /// Address the remote sees for us.
    pub observed_addr: Multiaddr,

    _priv: (),
}

// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `io::Error`.
fn parse_proto_msg(msg: impl AsRef<[u8]>) -> Result<(IdentifyInfo, Multiaddr), io::Error> {
    match structs_proto::Identify::decode(msg.as_ref()) {
        Ok(msg) => {
            // Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into
            // an `io::Error`.
            fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, io::Error> {
                Multiaddr::try_from(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
            }

            let listen_addrs = {
                let mut addrs = Vec::new();
                for addr in msg.listen_addrs.into_iter() {
                    addrs.push(bytes_to_multiaddr(addr)?);
                }
                addrs
            };

            let public_key = PublicKey::from_protobuf_encoding(&msg.public_key.unwrap_or_default())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let observed_addr = bytes_to_multiaddr(msg.observed_addr.unwrap_or_default())?;
            let info = IdentifyInfo {
                public_key,
                protocol_version: msg.protocol_version.unwrap_or_default(),
                agent_version: msg.agent_version.unwrap_or_default(),
                listen_addrs,
                protocols: msg.protocols,
            };

            Ok((info, observed_addr))
        }

        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }
}

pub(crate) async fn consume_message<T>(mut stream: Substream<T>) -> Result<RemoteInfo, TransportError>
where
    T: StreamInfo + ReadEx + WriteEx + Unpin + std::fmt::Debug + 'static,
{
    let buf = stream.read_one(4096).await?;
    stream.close2().await?;

    let (info, observed_addr) = match parse_proto_msg(&buf) {
        Ok(v) => v,
        Err(err) => {
            log::debug!("Failed to parse protobuf message; error = {:?}", err);
            return Err(err.into());
        }
    };

    log::trace!("Remote observes us as {:?}", observed_addr);
    log::trace!("Information received: {:?}", info);

    Ok(RemoteInfo {
        info,
        observed_addr: observed_addr.clone(),
        _priv: (),
    })
}

pub(crate) async fn produce_message<T>(mut stream: Substream<T>, info: IdentifyInfo) -> Result<(), TransportError>
where
    T: StreamInfo + ReadEx + WriteEx + Unpin + std::fmt::Debug + 'static,
{
    let listen_addrs = info.listen_addrs.into_iter().map(|addr| addr.to_vec()).collect();

    let pubkey_bytes = info.public_key.into_protobuf_encoding();

    //let observed_addr = stream.

    let message = structs_proto::Identify {
        agent_version: Some(info.agent_version),
        protocol_version: Some(info.protocol_version),
        public_key: Some(pubkey_bytes),
        listen_addrs: listen_addrs,
        observed_addr: None, //Some(observed_addr.to_vec()),
        protocols: info.protocols,
    };

    let mut bytes = Vec::with_capacity(message.encoded_len());
    message.encode(&mut bytes).expect("Vec<u8> provides capacity as needed");

    stream.write_one(&bytes).await?;

    stream.close2().await.map_err(io::Error::into)
}

/// Represents a prototype for the identify protocol.
///
/// The protocol works the following way:
///
/// - Server sends the identify message in protobuf to client
/// - Client receives the data and consume the data.
///
#[derive(Debug, Clone)]
pub(crate) struct IdentifyHandler {
    /// The information about ourselves.
    info: IdentifyInfo,
}

impl IdentifyHandler {
    pub(crate) fn new(pubkey: PublicKey, protocols: Vec<String>) -> Self {
        Self {
            info: IdentifyInfo {
                public_key: pubkey,
                protocol_version: "".to_string(),
                agent_version: "".to_string(),
                listen_addrs: vec![],
                protocols,
            },
        }
    }
}

impl UpgradeInfo for IdentifyHandler {
    type Info = &'static [u8];
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![IDENTIFY_PROTOCOL]
    }
}

#[async_trait]
impl<TSocket> ProtocolHandler<TSocket> for IdentifyHandler
where
    TSocket: StreamInfo + ReadEx + WriteEx + Unpin + std::fmt::Debug + 'static,
{
    /// The Ping handler's inbound protocol.
    /// Simply wait for any thing that coming in then send back
    async fn handle(&mut self, stream: Substream<TSocket>, _info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        log::trace!("Identify Protocol handling on {:?}", stream);

        let info = self.info.clone();
        log::trace!("Sending identify info to client: {:?}", info);

        produce_message(stream, info).await.map_err(|e| e.into())
    }

    fn box_clone(&self) -> IProtocolHandler<TSocket> {
        Box::new(self.clone())
    }
}

/// Represents a prototype for the identify push protocol.
///
/// This is just the opposite of the identify protocol.
/// It works the following way:
///
/// - Server receives the identify message in protobuf and consume it
/// - Client sends/pushes the the identify message to server side.
///
#[derive(Debug)]
pub(crate) struct IdentifyPushHandler<T> {
    tx: mpsc::UnboundedSender<SwarmEvent<T>>,
}

impl<T> Clone for IdentifyPushHandler<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<T: Send> IdentifyPushHandler<T> {
    pub(crate) fn new(tx: mpsc::UnboundedSender<SwarmEvent<T>>) -> Self {
        Self { tx }
    }
}

impl<T: Send> UpgradeInfo for IdentifyPushHandler<T> {
    type Info = &'static [u8];
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![IDENTIFY_PUSH_PROTOCOL]
    }
}

#[async_trait]
impl<TSocket, T> ProtocolHandler<TSocket> for IdentifyPushHandler<T>
where
    TSocket: StreamInfo + ReadEx + WriteEx + Unpin + std::fmt::Debug + 'static,
    T: Send + 'static,
{
    // receive the message and consume it
    async fn handle(&mut self, stream: Substream<TSocket>, _info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        let cid = stream.cid();
        log::trace!("Identify Push Protocol handling on {:?}", stream);

        let result = consume_message(stream).await.map_err(TransportError::into);

        let _ = self.tx.send(SwarmEvent::IdentifyResult { cid, result }).await;

        Ok(())
    }

    fn box_clone(&self) -> IProtocolHandler<TSocket> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::IdentifyHandler;
    use crate::connection::{ConnectionId, Direction};
    use crate::identify::{IdentifyInfo, IdentifyPushHandler};
    use crate::protocol_handler::ProtocolHandler;
    use crate::substream::Substream;
    use crate::{identify, SwarmEvent};
    use futures::channel::mpsc;
    use futures::{SinkExt, StreamExt};
    use libp2p_core::identity::Keypair;
    use libp2p_core::transport::TransportListener;
    use libp2p_core::upgrade::UpgradeInfo;
    use libp2p_core::{
        multiaddr::multiaddr,
        transport::{memory::MemoryTransport, Transport},
    };
    use rand::{thread_rng, Rng};

    #[test]
    fn produce_and_consume() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let listener_addr = mem_addr.clone();
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        let pubkey = Keypair::generate_ed25519_fixed().public();
        let key_cloned = pubkey.clone();

        async_std::task::spawn(async move {
            let socket = listener.accept().await.unwrap();
            let socket = Substream::new(socket, Direction::Inbound, b"", ConnectionId::default());

            let mut handler = IdentifyHandler::new(key_cloned, vec![]);
            let _ = handler.handle(socket, handler.protocol_info().first().unwrap()).await;
        });

        async_std::task::block_on(async move {
            let socket = MemoryTransport.dial(listener_addr).await.unwrap();
            let socket = Substream::new(socket, Direction::Inbound, b"", ConnectionId::default());

            let ri = identify::consume_message(socket).await.unwrap();
            assert_eq!(ri.info.public_key, pubkey);
        });
    }

    #[test]
    fn produce_and_consume_push() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let listener_addr = mem_addr.clone();
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        let pubkey = Keypair::generate_ed25519_fixed().public();
        let key_cloned = pubkey.clone();

        let (mut tx, mut rx) = mpsc::unbounded::<SwarmEvent<()>>();

        async_std::task::spawn(async move {
            let socket = MemoryTransport.dial(listener_addr).await.unwrap();
            let socket = Substream::new(socket, Direction::Inbound, b"", ConnectionId::default());

            let info = IdentifyInfo {
                public_key: key_cloned,
                protocol_version: "".to_string(),
                agent_version: "".to_string(),
                listen_addrs: vec![],
                protocols: vec![],
            };

            let _ = identify::produce_message(socket, info).await.unwrap();
        });

        async_std::task::block_on(async move {
            let socket = listener.accept().await.unwrap();
            let socket = Substream::new(socket, Direction::Inbound, b"", ConnectionId::default());

            let mut handler = IdentifyPushHandler::new(tx);
            let _ = handler.handle(socket, handler.protocol_info().first().unwrap()).await;

            let r = rx.next().await.unwrap();

            if let SwarmEvent::IdentifyResult { cid, result } = r {
                assert_eq!(result.unwrap().info.public_key, pubkey);
            } else {
                assert!(false);
            }

            //assert_eq!(ri.info.public_key, pubkey);
        });
    }
}

/*


#[test]
fn ping() {
    let cfg = IdentifyConfig::new().with_keep_alive(true);

    let (peer1_id, trans) = mk_transport();
    let mut swarm1 = Swarm::new(trans, Ping::new(cfg.clone()), peer1_id.clone());

    let (peer2_id, trans) = mk_transport();
    let mut swarm2 = Swarm::new(trans, Ping::new(cfg), peer2_id.clone());

    let (mut tx, mut rx) = mpsc::channel::<Multiaddr>(1);

    let pid1 = peer1_id.clone();
    let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    Swarm::listen_on(&mut swarm1, addr).unwrap();

    let peer1 = async move {
        while let Some(_) = swarm1.next().now_or_never() {}

        for l in Swarm::listeners(&swarm1) {
            tx.send(l.clone()).await.unwrap();
        }

        loop {
            match swarm1.next().await {
                PingEvent { peer, result: Ok(PingSuccess::Ping { rtt }) } => {
                    return (pid1.clone(), peer, rtt)
                },
                _ => {}
            }
        }
    };

    let pid2 = peer2_id.clone();
    let peer2 = async move {
        Swarm::dial_addr(&mut swarm2, rx.next().await.unwrap()).unwrap();

        loop {
            match swarm2.next().await {
                PingEvent { peer, result: Ok(PingSuccess::Ping { rtt }) } => {
                    return (pid2.clone(), peer, rtt)
                },
                _ => {}
            }
        }
    };

    let result = future::select(Box::pin(peer1), Box::pin(peer2));
    let ((p1, p2, rtt), _) = async_std::task::block_on(result).factor_first();
    assert!(p1 == peer1_id && p2 == peer2_id || p1 == peer2_id && p2 == peer1_id);
    assert!(rtt < Duration::from_millis(50));
}

fn mk_transport() -> (
    PeerId,
    Boxed<
        (PeerId, StreamMuxerBox),
        io::Error
    >
) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().into_peer_id();
    let transport = TcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(id_keys))
        .multiplex(libp2p_yamux::Config::default())
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .boxed();
    (peer_id, transport)
}


 */
