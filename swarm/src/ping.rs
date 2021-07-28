// Copyright 2018 Parity Technologies (UK) Ltd.
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
//! The [`PingHandler`] produces [`PingResult`]s, which will be consumed by the [`Swarm`]
//! , e.g. to close the [`Connection`].
//!
//! > **Note**: The ping protocol does not keep otherwise idle connections alive,
//! > it only adds an additional condition for terminating the connection, namely
//! > a certain number of failed ping requests.
//!
//! [`Swarm`]: libp2prs_swarm::Swarm
//! [`Transport`]: libp2p_core::Transport

use async_trait::async_trait;
use rand::{distributions, prelude::*};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use std::{error::Error, io};

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2prs_core::transport::TransportError;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_runtime::task;

use crate::connection::Connection;
use crate::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler};
use crate::substream::Substream;
use libp2prs_core::ProtocolId;

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
    /// The flag of Ping unsolicited. If true, Ping as soon as connection
    /// is established.
    unsolicited: bool,
    /// Whether the connection should generally be kept alive unless
    /// `max_failures` occur.
    keep_alive: bool,
}

impl Default for PingConfig {
    fn default() -> Self {
        Self::new()
    }
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
            unsolicited: true,
            keep_alive: false,
        }
    }
    /// Gets the ping timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sets the ping interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Sets the maximum number of consecutive ping failures upon which the remote
    /// peer is considered unreachable and the connection closed.
    pub fn max_failures(&self) -> u32 {
        self.max_failures.into()
    }
    /// Gets the unsolicited Ping flag.
    pub fn unsolicited(&self) -> bool {
        self.unsolicited
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
    /// Sets the unsolicited Ping flag.
    pub fn with_unsolicited(mut self, b: bool) -> Self {
        self.unsolicited = b;
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

pub async fn ping<T: AsyncRead + AsyncWrite + Send + Unpin + std::fmt::Debug>(
    mut stream: T,
    timeout: Duration,
) -> Result<Duration, TransportError> {
    let ping = async {
        let payload: [u8; PING_SIZE] = thread_rng().sample(distributions::Standard);
        log::trace!("Preparing ping payload {:?}", payload);

        stream.write_all(&payload).await?;
        let started = Instant::now();

        let mut recv_payload = [0u8; PING_SIZE];
        stream.read_exact(&mut recv_payload).await?;
        stream.close().await?;
        if recv_payload == payload {
            log::trace!("ping succeeded for {:?}", stream);
            Ok(started.elapsed())
        } else {
            log::info!("Invalid ping payload received {:?}", payload);
            Err(io::Error::new(io::ErrorKind::InvalidData, "Ping payload mismatch"))
        }
    };

    task::timeout(timeout, ping)
        .await
        .map_or(Err(TransportError::Timeout), |r| r.map_err(|e| e.into()))
}

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
#[derive(Debug, Clone)]
pub(crate) struct PingHandler {
    config: PingConfig,
}

impl PingHandler {
    pub(crate) fn new(config: PingConfig) -> Self {
        PingHandler { config }
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
pub(crate) const PING_PROTOCOL: &[u8] = b"/ipfs/ping/1.0.0";

const PING_SIZE: usize = 32;

impl UpgradeInfo for PingHandler {
    type Info = ProtocolId;
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![ProtocolId::new(PING_PROTOCOL, 0)]
    }
}

impl Notifiee for PingHandler {
    fn connected(&mut self, connection: &mut Connection) {
        let config = &self.config;
        if config.unsolicited() {
            log::trace!("starting Ping service for {:?}", connection);
            connection.start_ping(config.timeout(), config.interval(), config.max_failures());
        }
    }
}

#[async_trait]
impl ProtocolHandler for PingHandler {
    /// The Ping handler's inbound protocol.
    /// Simply wait for any thing that coming in then send back
    async fn handle(&mut self, mut stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
        log::trace!("Ping Protocol handling on {:?}", stream);

        let mut payload = [0u8; PING_SIZE];
        while stream.read_exact(&mut payload).await.is_ok() {
            stream.write_all(&payload).await?;
        }
        stream.close().await?;

        Ok(())
    }
    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::PingHandler;
    use crate::ping::{ping, PingConfig};
    use crate::protocol_handler::ProtocolHandler;
    use crate::substream::Substream;
    use libp2prs_core::transport::ListenerEvent;
    use libp2prs_core::upgrade::UpgradeInfo;
    use libp2prs_core::{
        multiaddr::multiaddr,
        transport::{memory::MemoryTransport, Transport},
    };
    use libp2prs_runtime::task;
    use rand::{thread_rng, Rng};
    use std::time::Duration;

    #[test]
    fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let listener_addr = mem_addr.clone();
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        task::spawn(async move {
            let socket = match listener.accept().await.unwrap() {
                ListenerEvent::Accepted(socket) => socket,
                _ => panic!("unreachable"),
            };
            let socket = Substream::new_with_default(Box::new(socket));

            let mut handler = PingHandler::new(PingConfig::new().with_unsolicited(true));
            let _ = handler.handle(socket, handler.protocol_info().first().unwrap().clone()).await;
        });

        task::block_on(async move {
            let socket = MemoryTransport.dial(listener_addr).await.unwrap();

            let rtt = ping(socket, Duration::from_secs(3)).await.unwrap();
            assert!(rtt > Duration::from_secs(0));
        });
    }
}
