// Copyright 2018 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use rand::{distributions, prelude::*};
use futures_timer::Delay;
use std::collections::VecDeque;
use std::time::Duration;
use std::num::NonZeroU32;
use libp2p_core::upgrade::{ProtocolName, UpgradeInfo};
use libp2p_swarm::SwarmError;
use libp2p_swarm::protocol_handler::{ProtocolHandler, BoxHandler};
use std::fmt;
use std::error::Error;
use libp2p_traits::{Read2, Write2};


/// The configuration for outbound pings.
#[derive(Clone, Debug)]
pub struct PingConfig {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between the last successful outbound or inbound ping
    /// and the next outbound ping.
    interval: Duration,
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
            timeout: Duration::from_secs(20),
            interval: Duration::from_secs(15),
            max_failures: NonZeroU32::new(1).expect("1 != 0"),
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

/// The result of an inbound or outbound ping.
pub type PingResult = Result<PingSuccess, PingFailure>;

/// The successful result of processing an inbound or outbound ping.
#[derive(Debug)]
pub enum PingSuccess {
    /// Received a ping and sent back a pong.
    Pong,
    /// Sent a ping and received back a pong.
    ///
    /// Includes the round-trip time.
    Ping { rtt: Duration },
}

/// An outbound ping failure.
#[derive(Debug)]
pub enum PingFailure {
    /// The ping timed out, i.e. no response was received within the
    /// configured ping timeout.
    Timeout,
    /// The ping failed for reasons other than a timeout.
    Other { error: Box<dyn std::error::Error + Send + 'static> }
}

impl fmt::Display for PingFailure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PingFailure::Timeout => f.write_str("Ping timeout"),
            PingFailure::Other { error } => write!(f, "Ping error: {}", error)
        }
    }
}

impl Error for PingFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PingFailure::Timeout => None,
            PingFailure::Other { error } => Some(&**error)
        }
    }
}

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
/// If the remote doesn't respond, produces an error that closes the connection.
#[derive(Debug, Clone)]
pub struct PingHandler {
    /// Configuration options.
    config: PingConfig,
/*    /// The timer for when to send the next ping.
    next_ping: Delay,
    /// The pending results from inbound or outbound pings, ready
    /// to be `poll()`ed.
    pending_results: VecDeque<PingResult>,*/
    /// The number of consecutive ping failures that occurred.
    failures: u32,
}

impl PingHandler {
    /// Builds a new `PingHandler` with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        PingHandler {
            config,
            // next_ping: Delay::new(Duration::new(0, 0)),
            // pending_results: VecDeque::with_capacity(2),
            failures: 0,
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


const PING_SIZE: usize = 32;

impl UpgradeInfo for PingHandler {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(b"/ipfs/ping/1.0.0")
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
        let mut msg = vec![0; 4096];
        loop {
            let n = stream.read2(&mut msg).await?;
            stream.write2(&msg[..n]).await?;
        }
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
