
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

use std::time::{Duration, Instant};
use std::num::NonZeroU32;
use std::io;
use std::convert::TryFrom;
use rand::{distributions, prelude::*};
use async_trait::async_trait;
use prost::Message;

use libp2p_traits::{Read2, Write2};
use libp2p_core::upgrade::{UpgradeInfo, ProtocolName};
use libp2p_core::transport::TransportError;

use crate::{SwarmError, ProtocolId};
use crate::protocol_handler::{ProtocolHandler, BoxHandler};
use libp2p_core::{PublicKey, Multiaddr};

mod structs_proto {
    include!(concat!(env!("OUT_DIR"), "/structs.rs"));
}


/// The configuration for outbound pings.
#[derive(Clone, Debug)]
pub struct IdentifyConfig;



// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `io::Error`.
fn parse_proto_msg(msg: impl AsRef<[u8]>) -> Result<(IdentifyInfo, Multiaddr), io::Error> {
    match structs_proto::Identify::decode(msg.as_ref()) {
        Ok(msg) => {
            // Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into
            // an `io::Error`.
            fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, io::Error> {
                Multiaddr::try_from(bytes)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
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
                protocols: msg.protocols
            };

            Ok((info, observed_addr))
        }

        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }
}

pub async fn identify<T: Read2 + Write2 + Send + std::fmt::Debug>(mut stream: T) -> Result<RemoteInfo, TransportError> {
    stream.close2().await?;
    //let msg = upgrade::read_one(&mut socket, 4096).await?;
    let mut buf = vec![0u8; 4096];
    let n = stream.read2(&mut buf).await?;
    let (info, observed_addr) = match parse_proto_msg(&buf[..n]) {
        Ok(v) => v,
        Err(err) => {
            log::debug!("Failed to parse protobuf message; error = {:?}", err);
            return Err(err.into())
        }
    };

    log::trace!("Remote observes us as {:?}", observed_addr);
    log::trace!("Information received: {:?}", info);

    Ok(RemoteInfo {
        info,
        observed_addr: observed_addr.clone(),
        _priv: ()
    })
}


/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
#[derive(Debug, Clone)]
pub struct IdentifyHandler {
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
            }
        }
    }
}

/// Represents a prototype for an upgrade to handle the ping protocol.
///
/// The protocol works the following way:
///
/// - Dialer sends 32 bytes of random data.
/// - Listener receives the data and sends it back.
/// - Dialer receives the data and verifies that it matches what it sent.
///
/// The dialer produces a `Duration`, which corresponds to the round-trip time
/// of the payload.
///
/// > **Note**: The round-trip time of a ping may be subject to delays induced
/// >           by the underlying transport, e.g. in the case of TCP there is
/// >           Nagle's algorithm, delayed acks and similar configuration options
/// >           which can affect latencies especially on otherwise low-volume
/// >           connections.
pub const IDENTIFY_PROTOCOL: &[u8] = b"/ipfs/id/1.0.0";

//pub const IDENTIFY_PUSH_PROTOCOL: &[u8] = b"/ipfs/id/1.0.0";


impl UpgradeInfo for IdentifyHandler {
    type Info = &'static [u8];
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(IDENTIFY_PROTOCOL)
    }
}

#[async_trait]
impl<C> ProtocolHandler<C> for IdentifyHandler
    where
        C: Read2 + Write2 + Unpin + Send + std::fmt::Debug + 'static
{
    /// The Ping handler's inbound protocol.
    /// Simply wait for any thing that coming in then send back
    async fn handle(&mut self, mut stream: C, _info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        log::trace!("Identify Protocol handling on {:?}", stream);

        let info = self.info.clone();
        log::trace!("Sending identify info to client: {:?}", info);

        let listen_addrs = info.listen_addrs
            .into_iter()
            .map(|addr| addr.to_vec())
            .collect();

        let pubkey_bytes = info.public_key.into_protobuf_encoding();

        let message = structs_proto::Identify {
            agent_version: Some(info.agent_version),
            protocol_version: Some(info.protocol_version),
            public_key: Some(pubkey_bytes),
            listen_addrs: listen_addrs,
            observed_addr: None,//Some(observed_addr.to_vec()),
            protocols: info.protocols
        };

        let mut bytes = Vec::with_capacity(message.encoded_len());
        message.encode(&mut bytes).expect("Vec<u8> provides capacity as needed");

        stream.write_all2(&bytes).await.map_err(|e|e.into())

        //upgrade::write_one(&mut self.inner, &bytes).await
    }

    fn box_clone(&self) -> BoxHandler<C> {
        Box::new(self.clone())
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

    _priv: ()
}


#[cfg(test)]
mod tests {
    use super::IdentifyHandler;
    use libp2p_core::{
        multiaddr::multiaddr,
        transport::{
            Transport,
            memory::MemoryTransport
        }
    };
    use rand::{thread_rng, Rng};
    use std::time::Duration;
    use libp2p_core::transport::TransportListener;
    use crate::protocol_handler::ProtocolHandler;
    use libp2p_core::upgrade::UpgradeInfo;
    use crate::ping::ping;

    #[test]
    fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let listener_addr = mem_addr.clone();
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        async_std::task::spawn(async move {
            let socket = listener.accept().await.unwrap();

            let mut handler = IdentifyHandler;
            let _ = handler.handle(socket, handler.protocol_info().first().unwrap()).await;
        });

        async_std::task::block_on(async move {
            let socket = MemoryTransport.dial(listener_addr).await.unwrap();

            let rtt = ping(socket, Duration::from_secs(3)).await.unwrap();
            assert!(rtt > Duration::from_secs(0));
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