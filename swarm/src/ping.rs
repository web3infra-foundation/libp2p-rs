

//! This module implements the `/ipfs/ping/1.0.0` protocol.
//!
//! The ping protocol can be used as a simple application-layer health check
//! for connections of any [`Transport`] as well as to measure and record
//! round-trip times.
//!
//! # Usage
//!
//! The [`PingHandler`] struct implements the [`ProtocolHandler`] trait. When used with a [`Swarm`],
//! it will respond to inbound ping requests and as necessary periodically send outbound
//! ping requests on every established connection. If a configurable number of pings fail,
//! the connection will be closed.
//!
//! The `Ping` network behaviour produces [`PingEvent`]s, which may be consumed from the `Swarm`
//! by an application, e.g. to collect statistics.
//!
//! > **Note**: The ping protocol does not keep otherwise idle connections alive,
//! > it only adds an additional condition for terminating the connection, namely
//! > a certain number of failed ping requests.
//!
//! [`Swarm`]: libp2p_swarm::Swarm
//! [`Transport`]: libp2p_core::Transport

use async_trait::async_trait;
use std::{collections::VecDeque, task::Context, task::Poll};
use std::time::{Duration, Instant};
use std::num::NonZeroU32;
use std::{fmt, io};
use std::error::Error;
use rand::{distributions, prelude::*};
use async_std::task;

use libp2p_core::PeerId;
use libp2p_traits::{Read2, Write2};
use libp2p_core::upgrade::UpgradeInfo;
use libp2p_core::transport::TransportError;

use crate::SwarmError;
use crate::protocol_handler::{ProtocolHandler, BoxHandler};


/// `Ping` is a [`NetworkBehaviour`] that responds to inbound pings and
/// periodically sends outbound pings on every established connection.
///
/// See the crate root documentation for more information.
pub struct Ping {
    /// Configuration for outbound pings.
    config: PingConfig,
}

impl Ping {
    /// Creates a new `Ping` network behaviour with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        Ping {
            config,
        }
    }
}

impl Default for Ping {
    fn default() -> Self {
        Ping::new(PingConfig::new())
    }
}

/// The configuration for outbound pings.
#[derive(Clone, Debug)]
pub struct PingConfig {
    /// The timeout of an outbound ping.
    pub(crate) timeout: Duration,
    /// The duration between the last successful outbound or inbound ping
    /// and the next outbound ping.
    pub(crate) interval: Duration,
    /// The maximum number of failed outbound pings before the associated
    /// connection is deemed unhealthy, indicating to the `Swarm` that it
    /// should be closed.
    max_failures: NonZeroU32,
    /// Whether the connection should generally be kept alive unless
    /// `max_failures` occur.
    keep_alive: bool,
}

impl PingConfig {
    /// Creates a new `PingConfig` with the following default settings:
    ///
    ///   * [`PingConfig::with_interval`] 15s
    ///   * [`PingConfig::with_timeout`] 20s
    ///   * [`PingConfig::with_max_failures`] 1
    ///   * [`PingConfig::with_keep_alive`] false
    ///
    /// These settings have the following effect:
    ///
    ///   * A ping is sent every 15 seconds on a healthy connection.
    ///   * Every ping sent must yield a response within 20 seconds in order to
    ///     be successful.
    ///   * A single ping failure is sufficient for the connection to be subject
    ///     to being closed.
    ///   * The connection may be closed at any time as far as the ping protocol
    ///     is concerned, i.e. the ping protocol itself does not keep the
    ///     connection alive.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            interval: Duration::from_secs(5),
            max_failures: NonZeroU32::new(3).expect("1 != 0"),
            keep_alive: false
        }
    }

    /// Sets the ping timeout.
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// Sets the ping interval.
    pub fn with_interval(mut self, d: Duration) -> Self {
        self.interval = d;
        self
    }

    /// Sets the maximum number of consecutive ping failures upon which the remote
    /// peer is considered unreachable and the connection closed.
    pub fn with_max_failures(mut self, n: NonZeroU32) -> Self {
        self.max_failures = n;
        self
    }

    /// Sets whether the ping protocol itself should keep the connection alive,
    /// apart from the maximum allowed failures.
    ///
    /// By default, the ping protocol itself allows the connection to be closed
    /// at any time, i.e. in the absence of ping failures the connection lifetime
    /// is determined by other protocol handlers.
    ///
    /// If the maximum number of allowed ping failures is reached, the
    /// connection is always terminated as a result of [`ProtocolsHandler::poll`]
    /// returning an error, regardless of the keep-alive setting.
    pub fn with_keep_alive(mut self, b: bool) -> Self {
        self.keep_alive = b;
        self
    }
}

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
/// If the remote doesn't respond, produces an error that closes the connection.
///
///
///
///
///
pub struct PingService {
    /// Configuration options.
    pub(crate) config: PingConfig,
    /// The number of consecutive ping failures that occurred.
    failures: u32,
}

impl PingService {
    /// Builds a new `PingHandler` with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        PingService {
            config,
            failures: 0,
        }
    }

    pub(crate) fn nothing(&self) {}
}


pub async fn ping<T: Read2 + Write2 + Send + std::fmt::Debug>(mut stream: T, timeout: Duration) -> Result<Duration, TransportError> {

    let ping = async {
        let payload: [u8; PING_SIZE] = thread_rng().sample(distributions::Standard);
        log::trace!("Preparing ping payload {:?}", payload);

        stream.write_all2(&payload).await?;
        stream.close2().await?;
        let started = Instant::now();

        let mut recv_payload = [0u8; PING_SIZE];
        stream.read_exact2(&mut recv_payload).await?;
        if recv_payload == payload {
            log::trace!("ping succeeded for {:?}", stream);
            Ok(started.elapsed())
        } else {
            log::info!("Invalid ping payload received {:?}", payload);
            Err(io::Error::new(io::ErrorKind::InvalidData, "Ping payload mismatch"))
        }
    };

    async_std::io::timeout(timeout, ping).await.map_err(|e| e.into())
}





/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
#[derive(Debug, Clone)]
pub struct PingHandler;


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


const PING_SIZE: usize = 32;

pub const PING_PROTOCOL: &[u8] = b"/ipfs/ping/1.0.0";

impl UpgradeInfo for PingHandler {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(PING_PROTOCOL)
    }
}

#[async_trait]
impl<C> ProtocolHandler<C> for PingHandler
    where
        C: Read2 + Write2 + Unpin + Send + std::fmt::Debug + 'static
{
    /// The Ping handler's inbound protocol.
    /// Simply wait for any thing that coming in then send back
    async fn handle(&mut self, mut stream: C, info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        log::trace!("Ping Protocol handling on {:?}", stream);

        let mut payload = [0u8; PING_SIZE];
        while let Ok(_) = stream.read_exact2(&mut payload).await {
            stream.write_all2(&payload).await?;
        }
        stream.close2().await?;

        Ok(())
    }
    fn box_clone(&self) -> BoxHandler<C> {
        Box::new(self.clone())
    }
}


#[cfg(test)]
mod tests {
    use super::PingHandler;
    use libp2p_core::{
        upgrade,
        multiaddr::multiaddr,
        transport::{
            Transport,
            ListenerEvent,
            memory::MemoryTransport
        }
    };
    use rand::{thread_rng, Rng};
    use std::time::Duration;

    #[test]
    fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        async_std::task::spawn(async move {
            // let listener_event = listener.next().await.unwrap();
            // let (listener_upgrade, _) = listener_event.unwrap().into_upgrade().unwrap();
            // let conn = listener_upgrade.await.unwrap();
            // upgrade::apply_inbound(conn, Ping::default()).await.unwrap();
        });

        async_std::task::block_on(async move {
            // let c = MemoryTransport.dial(listener_addr).unwrap().await.unwrap();
            // let rtt = upgrade::apply_outbound(c, Ping::default(), upgrade::Version::V1).await.unwrap();
            // assert!(rtt > Duration::from_secs(0));
        });
    }
}



/*


#[test]
fn ping() {
    let cfg = PingConfig::new().with_keep_alive(true);

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