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

//! Communication channel to the remote peer.
//!
//! The Swarm [`Connection`] is the multiplexed connection, which can be used to open or accept
//! new substreams. Furthermore, a raw substream opened by the StreamMuxer has to be upgraded to
//! the Swarm [`Substream`] via multistream select procedure.
//!

use smallvec::SmallVec;
use std::fmt;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::prelude::*;

use libp2prs_runtime::task;

use libp2prs_core::identity::Keypair;
use libp2prs_core::multistream::Negotiator;
use libp2prs_core::muxing::IStreamMuxer;
use libp2prs_core::transport::TransportError;
use libp2prs_core::PublicKey;

use crate::connection::Direction::Outbound;
use crate::control::SwarmControlCmd;
use crate::identify::{IDENTIFY_PROTOCOL, IDENTIFY_PUSH_PROTOCOL};
use crate::metrics::metric::Metric;
use crate::ping::PING_PROTOCOL;
use crate::substream::{ConnectInfo, StreamId, Substream, SubstreamView};
use crate::{identify, ping, Multiaddr, PeerId, ProtocolId, SwarmError, SwarmEvent};

/// The direction of a peer-to-peer communication channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    /// The socket comes from a dialer.
    Outbound,
    /// The socket comes from a listener.
    Inbound,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if self == &Outbound { "Out" } else { "In " })
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(usize);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<5}", self.0)
    }
}

/// A multiplexed connection to a peer with associated `Substream`s.
#[allow(dead_code)]
pub struct Connection {
    /// The unique ID for a connection
    id: ConnectionId,
    /// Node that handles the stream_muxer.
    stream_muxer: IStreamMuxer,
    /// The tx channel, to send Connection events to Swarm
    tx: mpsc::UnboundedSender<SwarmEvent>,
    /// The ctrl tx channel.
    ctrl: mpsc::Sender<SwarmControlCmd>,
    /// All sub-streams belonged to this connection.
    substreams: SmallVec<[SubstreamView; 8]>,
    /// Direction of this connection
    dir: Direction,
    /// Indicates if Ping runtime is running.
    ping_running: Arc<AtomicBool>,
    /// Ping failure count.
    ping_failures: u32,
    /// Identity service
    identity: Option<()>,
    /// The runtime handle of this connection, returned by runtime::Spawn
    /// handle.await() when closing a connection
    handle: Option<task::TaskHandle<()>>,
    /// The runtime handle of the Ping service of this connection
    ping_handle: Option<task::TaskHandle<()>>,
    /// The runtime handle of the Identify service of this connection
    identify_handle: Option<task::TaskHandle<()>>,
    /// The runtime handle of the Identify Push service of this connection
    identify_push_handle: Option<task::TaskHandle<()>>,
    /// Global metrics.
    metric: Arc<Metric>,
    /// Flag, means that current connection is closed or not.
    closing: bool,
}

impl PartialEq for Connection {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("muxer", &self.stream_muxer)
            .field("dir", &self.dir)
            .field("subs", &self.substreams)
            .finish()
    }
}

//impl Unpin for Connection where TMuxer: StreamMuxer {}

#[allow(dead_code)]
impl Connection {
    /// Builds a new `Connection` from the given substream multiplexer
    /// and a tx channel which will used to send events to Swarm.
    pub(crate) fn new(
        id: usize,
        stream_muxer: IStreamMuxer,
        dir: Direction,
        tx: mpsc::UnboundedSender<SwarmEvent>,
        ctrl: mpsc::Sender<SwarmControlCmd>,
        metric: Arc<Metric>,
    ) -> Self {
        Connection {
            id: ConnectionId(id),
            stream_muxer,
            tx,
            ctrl,
            dir,
            substreams: Default::default(),
            handle: None,
            ping_running: Arc::new(AtomicBool::new(false)),
            ping_failures: 0,
            ping_handle: None,
            identity: None,
            identify_handle: None,
            identify_push_handle: None,
            metric,
            closing: false,
        }
    }

    pub(crate) fn to_view(&self) -> ConnectionView {
        ConnectionView {
            id: self.id,
            dir: self.dir,
            info: self.info(),
            substreams: self.substreams.clone(),
        }
    }

    pub(crate) fn substream_view(&self) -> Vec<SubstreamView> {
        self.substreams.to_vec()
    }

    /// Returns the unique Id of the connection.
    pub(crate) fn id(&self) -> ConnectionId {
        self.id
    }

    /// Returns a reference of the stream_muxer.
    pub(crate) fn stream_muxer(&self) -> &IStreamMuxer {
        &self.stream_muxer
    }

    /// Sets the runtime handle of the connection.
    pub(crate) fn set_handle(&mut self, handle: task::TaskHandle<()>) {
        self.handle = Some(handle);
    }

    /// Opens a sub stream with the protocols specified
    pub fn open_stream<T: Send + 'static>(
        &mut self,
        pids: Vec<ProtocolId>,
        f: impl FnOnce(Result<Substream, TransportError>) -> T + Send + 'static,
    ) -> task::TaskHandle<()> {
        let cid = self.id();
        let stream_muxer = self.stream_muxer().box_clone();
        let mut tx = self.tx.clone();
        let ctrl = self.ctrl.clone();
        let metric = self.metric.clone();

        task::spawn(async move {
            let result = open_stream_internal(cid, stream_muxer, pids, ctrl, metric).await;

            // TODO: how to extract the error from TransportError, ??? it doesn't implement 'Clone'
            // So, at this moment, make a new 'TransportError::Internal'
            let event = match result.as_ref() {
                Ok(sub_stream) => {
                    let view = sub_stream.to_view();
                    SwarmEvent::StreamOpened { view }
                }
                Err(_) => SwarmEvent::StreamError {
                    cid,
                    dir: Direction::Outbound,
                    error: TransportError::Internal,
                },
            };

            let _ = tx.send(event).await;

            f(result);
        })
    }

    pub fn is_closing(&self) -> bool {
        self.closing
    }

    /// Closes the inner stream_muxer. Spawn a runtime to avoid blocking.
    pub fn close(&mut self) {
        log::debug!("closing {:?}", self);

        let mut stream_muxer = self.stream_muxer.clone();
        // spawns a runtime to close the stream_muxer, later connection will cleaned up
        // in 'handle_connection_closed'
        self.closing = true;
        task::spawn(async move {
            let _ = stream_muxer.close().await;
        });
    }

    /// Waits for bg-runtime & accept-runtime.
    pub(crate) async fn wait(&mut self) -> Result<(), SwarmError> {
        // wait for accept-runtime and bg-runtime to exit
        if let Some(h) = self.handle.take() {
            h.await;
        }
        Ok(())
    }
    /// local_addr is the multiaddr on our side of the connection.
    pub(crate) fn local_addr(&self) -> Multiaddr {
        self.stream_muxer.local_multiaddr()
    }

    /// remote_addr is the multiaddr on the remote side of the connection.
    pub(crate) fn remote_addr(&self) -> Multiaddr {
        self.stream_muxer.remote_multiaddr()
    }

    /// local_peer is the Peer on our side of the connection.
    pub(crate) fn local_peer(&self) -> PeerId {
        self.stream_muxer.local_peer()
    }

    /// remote_peer is the Peer on the remote side.
    pub fn remote_peer(&self) -> PeerId {
        self.stream_muxer.remote_peer()
    }

    /// local_priv_key is the public key of the peer on this side.
    pub(crate) fn local_priv_key(&self) -> Keypair {
        self.stream_muxer.local_priv_key()
    }

    /// remote_pub_key is the public key of the peer on the remote side.
    pub(crate) fn remote_pub_key(&self) -> PublicKey {
        self.stream_muxer.remote_pub_key()
    }

    /// Adds a substream id to the list.
    pub(crate) fn add_stream(&mut self, view: SubstreamView) {
        log::debug!("adding sub {:?} to connection", view);
        self.substreams.push(view);
    }
    /// Removes a substream id from the list.
    pub(crate) fn del_stream(&mut self, sid: StreamId) {
        log::debug!("removing sub {:?} from connection", sid);
        self.substreams.retain(|s| s.id != sid);
    }

    /// Returns how many substreams in the list.
    pub(crate) fn num_streams(&self) -> usize {
        self.substreams.len()
    }

    /// Starts the Ping service on this connection. The runtime handle will be tracked
    /// by the connection for later closing the Ping service
    ///
    /// Note that we don't generate StreamOpened/Closed event for Ping/Identify outbound
    /// simply because it doesn't make much sense doing so for a transient outgoing
    /// stream.
    pub(crate) fn start_ping(&mut self, timeout: Duration, interval: Duration, max_failures: u32) {
        self.ping_running.store(true, Ordering::Relaxed);

        let cid = self.id();
        let stream_muxer = self.stream_muxer.clone();
        let mut tx = self.tx.clone();
        let flag = self.ping_running.clone();
        let pids = vec![PING_PROTOCOL.into()];
        let ctrl = self.ctrl.clone();
        let metric = self.metric.clone();

        let handle = task::spawn(async move {
            let mut fail_cnt: u32 = 0;
            loop {
                if !flag.load(Ordering::Relaxed) {
                    break;
                }

                // sleep for the interval
                task::sleep(interval).await;

                //recheck, in case ping service has been terminated already
                if !flag.load(Ordering::Relaxed) {
                    break;
                }

                let stream_muxer = stream_muxer.clone();
                let pids = pids.clone();

                let ctrl2 = ctrl.clone();
                let r = open_stream_internal(cid, stream_muxer, pids, ctrl2, metric.clone()).await;
                let r = match r {
                    Ok(stream) => {
                        let view = stream.to_view();
                        let _ = tx.send(SwarmEvent::StreamOpened { view }).await;
                        let res = ping::ping(stream, timeout).await;
                        if res.is_ok() {
                            fail_cnt = 0;
                        } else {
                            fail_cnt += 1;
                        }
                        res
                    }
                    Err(err) => {
                        // looks like the peer doesn't support the protocol
                        log::info!("Ping protocol not supported: {:?} {:?}", cid, err);
                        let _ = tx
                            .send(SwarmEvent::StreamError {
                                cid,
                                dir: Direction::Outbound,
                                error: TransportError::Internal,
                            })
                            .await;

                        Err(err)
                    }
                };

                if fail_cnt >= max_failures {
                    let _ = tx
                        .send(SwarmEvent::PingResult {
                            cid,
                            result: r.map_err(|e| e.into()),
                        })
                        .await;

                    break;
                }
            }

            log::debug!("ping runtime exiting...");
        });

        self.ping_handle = Some(handle);
    }

    /// Stops the Ping service on this connection
    pub(crate) async fn stop_ping(&mut self) {
        if let Some(h) = self.ping_handle.take() {
            log::debug!("stopping Ping service for {:?}...", self.id);
            self.ping_running.store(false, Ordering::Relaxed);
            h.await;
            //h.cancel().await;
        }
    }

    /// Starts the Identify service on this connection.
    pub(crate) fn start_identify(&mut self) {
        let cid = self.id();
        let stream_muxer = self.stream_muxer.clone();
        let mut tx = self.tx.clone();
        let ctrl = self.ctrl.clone();
        let pids = vec![IDENTIFY_PROTOCOL.into()];
        let metric = self.metric.clone();

        let handle = task::spawn(async move {
            let r = open_stream_internal(cid, stream_muxer, pids, ctrl, metric).await;
            let r = match r {
                Ok(stream) => {
                    let view = stream.to_view();
                    let _ = tx.send(SwarmEvent::StreamOpened { view }).await;
                    identify::process_message(stream).await
                }
                Err(err) => {
                    // looks like the peer doesn't support the protocol
                    log::info!("Identify protocol not supported: {:?} {:?}", cid, err);
                    let _ = tx
                        .send(SwarmEvent::StreamError {
                            cid,
                            dir: Direction::Outbound,
                            error: TransportError::Internal,
                        })
                        .await;

                    Err(err)
                }
            };
            let _ = tx
                .send(SwarmEvent::IdentifyResult {
                    cid,
                    result: r.map_err(TransportError::into),
                })
                .await;

            log::debug!("identify runtime exiting...");
        });

        self.identify_handle = Some(handle);
    }

    pub(crate) async fn stop_identify(&mut self) {
        if let Some(h) = self.identify_handle.take() {
            log::debug!("stopping Identify service for {:?}...", self.id);
            h.cancel().await;
        }
    }

    /// Starts the Identify service on this connection.
    pub(crate) fn start_identify_push(&mut self) {
        let cid = self.id();
        let stream_muxer = self.stream_muxer.clone();
        let pids = vec![IDENTIFY_PUSH_PROTOCOL.into()];
        let metric = self.metric.clone();

        let mut ctrl = self.ctrl.clone();

        let mut tx = self.tx.clone();

        let handle = task::spawn(async move {
            let (swrm_tx, swrm_rx) = oneshot::channel();
            if ctrl.send(SwarmControlCmd::IdentifyInfo(swrm_tx)).await.is_err() {
                // this might happen, when swarm is exiting...
                return;
            }
            let info = swrm_rx.await.expect("get identify info");

            let r = open_stream_internal(cid, stream_muxer, pids, ctrl, metric).await;
            match r {
                Ok(stream) => {
                    let view = stream.to_view();
                    let _ = tx.send(SwarmEvent::StreamOpened { view }).await;
                    // ignore the error
                    let _ = identify::produce_message(stream, info).await;
                }
                Err(err) => {
                    // looks like the peer doesn't support the protocol
                    log::info!("Identify push protocol not supported: {:?} {:?}", cid, err);
                    let _ = tx
                        .send(SwarmEvent::StreamError {
                            cid,
                            dir: Direction::Outbound,
                            error: TransportError::Internal,
                        })
                        .await;
                }
            }

            log::debug!("identify push runtime exiting...");
        });

        self.identify_push_handle = Some(handle);
    }
    pub(crate) async fn stop_identify_push(&mut self) {
        if let Some(h) = self.identify_push_handle.take() {
            log::debug!("stopping Identify Push service for {:?}...", self.id);
            h.cancel().await;
        }
    }

    pub(crate) fn info(&self) -> ConnectionInfo {
        // calculate inbound
        let num_inbound_streams = self.substreams.iter().fold(0usize, |mut acc, s| {
            if s.dir == Direction::Inbound {
                acc += 1;
            }
            acc
        });
        let num_outbound_streams = self.substreams.len() - num_inbound_streams;
        ConnectionInfo {
            la: self.local_addr(),
            ra: self.remote_addr(),
            local_peer_id: self.local_peer(),
            remote_peer_id: self.remote_peer(),
            num_inbound_streams,
            num_outbound_streams,
        }
    }
}

#[derive(Debug)]
/// ConnectionView is used for debugging purpose.
pub struct ConnectionView {
    /// The unique ID for a connection.
    pub id: ConnectionId,
    /// Direction of this connection.
    pub dir: Direction,
    /// Detailed information of this connection.
    pub info: ConnectionInfo,
    /// Handler that processes substreams.
    pub substreams: SmallVec<[SubstreamView; 8]>,
}

impl fmt::Display for ConnectionView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} RPID({:52}) I/O({}{}) RA({})",
            self.id, self.dir, self.info.remote_peer_id, self.info.num_inbound_streams, self.info.num_outbound_streams, self.info.ra
        )
    }
}

async fn open_stream_internal(
    cid: ConnectionId,
    mut stream_muxer: IStreamMuxer,
    pids: Vec<ProtocolId>,
    ctrl: mpsc::Sender<SwarmControlCmd>,
    metric: Arc<Metric>,
) -> Result<Substream, TransportError> {
    log::debug!("opening substream on {:?} {:?}", cid, pids);

    let raw_stream = stream_muxer.open_stream().await?;
    let la = stream_muxer.local_multiaddr();
    let ra = stream_muxer.remote_multiaddr();
    let rpid = stream_muxer.remote_peer();

    // now it's time to do protocol multiplexing for sub stream
    let negotiator = Negotiator::new_with_protocols(pids);
    let result = negotiator.select_one(raw_stream).await;

    match result {
        Ok((proto, raw_stream)) => {
            log::debug!("selected outbound {:?} {:?}", cid, proto);

            let ci = ConnectInfo { la, ra, rpid };
            let stream = Substream::new(raw_stream, metric.clone(), Direction::Outbound, proto, cid, ci, ctrl);
            Ok(stream)
        }
        Err(err) => {
            log::debug!("failed outbound protocol selection {:?} {:?}", cid, err);
            Err(TransportError::NegotiationError(err))
        }
    }
}

/// Information about the network obtained by [`Network::info()`].
#[derive(Debug)]
pub struct ConnectionInfo {
    /// The local multiaddr of this connection.
    pub la: Multiaddr,
    /// The remote multiaddr of this connection.
    pub ra: Multiaddr,
    /// The local peer ID.
    pub local_peer_id: PeerId,
    /// The remote peer ID.
    pub remote_peer_id: PeerId,
    /// The total number of inbound sub streams.
    pub num_inbound_streams: usize,
    /// The total number of outbound sub streams.
    pub num_outbound_streams: usize,
    // /// The Sub-streams.
    // pub streams: Vec<StreamStats>,
}
