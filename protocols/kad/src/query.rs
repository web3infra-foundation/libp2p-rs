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

use futures::channel::mpsc;
use futures::future::Either;
use futures::{FutureExt, SinkExt, StreamExt};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering, AtomicU64};
use std::sync::Arc;
use std::{num::NonZeroUsize, time::Duration, time::Instant};

use libp2prs_core::peerstore::{PROVIDER_ADDR_TTL, TEMP_ADDR_TTL};
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_runtime::task;
use libp2prs_swarm::Control as SwarmControl;

use crate::kbucket::{Distance, Key};
use crate::{record, KadError, ALPHA_VALUE, BETA_VALUE, K_VALUE};

use crate::kad::{KadPoster, MessageStats, MessengerManager, Ledger};
use crate::protocol::{KadConnectionType, KadPeer, ProtocolEvent};
use crate::task_limit::TaskLimiter;
use libp2prs_core::metricmap::MetricMap;
// use std::ops::Add;

/// Execution statistics of a query.
#[derive(Debug, Clone, Default)]
pub struct IterativeStats {
    pub(crate) requests: u32,
    pub(crate) success: u32,
    pub(crate) failure: u32,
}

// Atomic version of QueryStats.
#[derive(Default)]
pub struct IterativeStatsAtomic {
    pub(crate) requests: AtomicU32,
    pub(crate) success: AtomicU32,
    pub(crate) failure: AtomicU32,
}

impl IterativeStatsAtomic {
    pub(crate) fn to_view(&self) -> IterativeStats {
        IterativeStats {
            requests: self.requests.load(Ordering::Relaxed),
            success: self.success.load(Ordering::Relaxed),
            failure: self.failure.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct QueryStats {
    pub(crate) fixed_query_executed: usize,
    pub(crate) fixed_query_completed: usize,
    pub(crate) iter_query_executed: usize,
    pub(crate) iter_query_completed: usize,
    pub(crate) iter_query_timeout: usize,
    // tx error of fixed query
    pub(crate) fixed_tx_error: usize,
    // details of iterative query
    pub(crate) iterative: IterativeStats,
    // Kad message tx
    pub(crate) message_tx: MessageStats,
}

#[derive(Default)]
pub(crate) struct QueryStatsAtomic {
    pub(crate) fixed_query_executed: AtomicUsize,
    pub(crate) fixed_query_completed: AtomicUsize,
    pub(crate) iter_query_executed: AtomicUsize,
    pub(crate) iter_query_completed: AtomicUsize,
    pub(crate) iter_query_timeout: AtomicUsize,
    // tx error for fixed query
    pub(crate) fixed_tx_error: AtomicUsize,
    // details of iterative query
    pub(crate) iterative: IterativeStatsAtomic,
    // message tx
    pub(crate) message_tx: MessageStatsAtomic,
}

impl QueryStatsAtomic {
    pub(crate) fn to_view(&self) -> QueryStats {
        QueryStats {
            fixed_query_executed: self.fixed_query_executed.load(Ordering::Relaxed),
            fixed_query_completed: self.fixed_query_completed.load(Ordering::Relaxed),
            iter_query_executed: self.iter_query_executed.load(Ordering::Relaxed),
            iter_query_completed: self.iter_query_completed.load(Ordering::Relaxed),
            iter_query_timeout: self.iter_query_timeout.load(Ordering::Relaxed),
            fixed_tx_error: self.fixed_tx_error.load(Ordering::Relaxed),
            iterative: self.iterative.to_view(),
            message_tx: self.message_tx.to_view(),
        }
    }
}

#[derive(Default)]
pub struct MessageStatsAtomic {
    pub(crate) ping: AtomicUsize,
    pub(crate) find_node: AtomicUsize,
    pub(crate) get_provider: AtomicUsize,
    pub(crate) add_provider: AtomicUsize,
    pub(crate) get_value: AtomicUsize,
    pub(crate) put_value: AtomicUsize,
}

impl MessageStatsAtomic {
    pub(crate) fn to_view(&self) -> MessageStats {
        MessageStats {
            ping: self.ping.load(Ordering::Relaxed),
            find_node: self.find_node.load(Ordering::Relaxed),
            get_provider: self.get_provider.load(Ordering::Relaxed),
            add_provider: self.add_provider.load(Ordering::Relaxed),
            get_value: self.get_value.load(Ordering::Relaxed),
            put_value: self.put_value.load(Ordering::Relaxed),
        }
    }
}

type Result<T> = std::result::Result<T, KadError>;

/// The configuration for queries in a `QueryPool`.
#[derive(Debug, Clone)]
pub struct QueryConfig {
    /// Timeout of a single query.
    pub timeout: Duration,

    /// The k value to use.
    pub k_value: NonZeroUsize,

    /// Alpha value for iterative queries.
    pub alpha_value: NonZeroUsize,

    /// The number of peers closest to a target that must have responded
    /// for an iterative query to terminate.
    pub beta_value: NonZeroUsize,
}

impl Default for QueryConfig {
    fn default() -> Self {
        QueryConfig {
            timeout: Duration::from_secs(180),
            k_value: K_VALUE,
            alpha_value: ALPHA_VALUE,
            beta_value: BETA_VALUE,
        }
    }
}

/// The fixed query.
///
/// It simply use the fixed peers for PutValue/AddProvider operations.
/// The alpha value is used to for maximum allowed tasks for messaging.
pub(crate) struct FixedQuery {
    /// The Messenger is used to send/receive Kad messages.
    messengers: MessengerManager,
    /// The query type to be executed.
    query_type: QueryType,
    /// The kad configurations for queries.
    config: QueryConfig,
    /// The seed peers used to start the query.
    peers: Vec<PeerId>,
    /// The statistics.
    stats: Arc<QueryStatsAtomic>,
}

impl FixedQuery {
    pub(crate) fn new(
        query_type: QueryType,
        messengers: MessengerManager,
        config: QueryConfig,
        peers: Vec<PeerId>,
        stats: Arc<QueryStatsAtomic>,
    ) -> Self {
        Self {
            messengers,
            query_type,
            config,
            peers,
            stats,
        }
    }

    pub(crate) fn run<F>(self, f: F)
        where
            F: FnOnce(Result<()>) + Send + 'static,
    {
        log::debug!("run fixed query {:?}", self.query_type);

        // check for empty fixed peers
        if self.peers.is_empty() {
            log::info!("no fixed peers, abort running");
            f(Err(KadError::NoKnownPeers));
            return;
        }

        // update stats
        let stats = self.stats.clone();
        stats.fixed_query_executed.fetch_add(1, Ordering::SeqCst);

        let me = self;
        let mut limiter = TaskLimiter::new(me.config.alpha_value);
        let message_stats = stats.clone();
        task::spawn(async move {
            for peer in me.peers {
                // let record = record.clone();
                let mut messengers = me.messengers.clone();
                let qt = me.query_type.clone();
                let stats = message_stats.clone();

                limiter
                    .run(async move {
                        let mut ms = messengers.get_messenger(&peer).await?;
                        match qt {
                            QueryType::PutRecord { record } => {
                                let _ = ms.send_put_value(record).await?;
                                stats.message_tx.put_value.fetch_add(1, Ordering::SeqCst);
                            }
                            QueryType::AddProvider { provider, addresses } => {
                                let _ = ms.send_add_provider(provider, addresses).await?;
                                stats.message_tx.add_provider.fetch_add(1, Ordering::SeqCst);
                            }
                            _ => panic!("BUG"),
                        }
                        messengers.put_messenger(ms);

                        Ok::<(), KadError>(())
                    })
                    .await;
            }
            let (c, s) = limiter.wait().await;
            log::info!("data announced to total {} peers, success={}", c, s);

            // update tx error
            stats.fixed_tx_error.fetch_add(c - s, Ordering::SeqCst);
            // update stats
            stats.fixed_query_completed.fetch_add(1, Ordering::SeqCst);

            f(Ok(()))
        });
    }
}

struct QueryJob {
    key: record::Key,
    qt: QueryType,
    messengers: MessengerManager,
    peer: PeerId,
    addrs: Vec<Multiaddr>,
    stats: Arc<QueryStatsAtomic>,
    ledgers: Arc<MetricMap<PeerId, Ledger>>,
    tx: mpsc::Sender<QueryUpdate>,
}

impl QueryJob {
    async fn execute(self) -> Result<()> {
        let mut me = self;
        let startup = Instant::now();

        let mut messenger = me.messengers.get_messenger(&me.peer).await?;
        let peer = me.peer;
        match me.qt {
            QueryType::GetClosestPeers | QueryType::FindPeer => {
                let closer = messenger.send_find_node(me.key).await?;
                me.stats.message_tx.find_node.fetch_add(1, Ordering::SeqCst);
                let duration = startup.elapsed();
                let _ = me
                    .tx
                    .send(QueryUpdate::Queried {
                        source: peer,
                        closer,
                        provider: None,
                        record: None,
                        duration,
                    })
                    .await;
            }
            QueryType::GetProviders { .. } => {
                let (closer, provider) = messenger.send_get_providers(me.key).await?;
                me.stats.message_tx.get_provider.fetch_add(1, Ordering::SeqCst);
                let duration = startup.elapsed();
                let _ = me
                    .tx
                    .send(QueryUpdate::Queried {
                        source: peer,
                        closer,
                        provider: Some(provider),
                        record: None,
                        duration,
                    })
                    .await;
            }
            QueryType::GetRecord { .. } => {
                let (closer, record) = messenger.send_get_value(me.key).await?;
                me.stats.message_tx.get_value.fetch_add(1, Ordering::SeqCst);
                let duration = startup.elapsed();
                let _ = me
                    .tx
                    .send(QueryUpdate::Queried {
                        source: peer,
                        closer,
                        provider: None,
                        record,
                        duration,
                    })
                    .await;
            }
            _ => panic!("not gonna happen"),
        }

        // try to put messenger into cache
        me.messengers.put_messenger(messenger);

        Ok(())
    }
}

/// Representation of a peer in the context of a iterator.
#[derive(Debug, Clone)]
struct PeerWithState {
    peer: KadPeer,
    state: PeerState,
}

/// The state of a single `PeerWithState`.
#[derive(Debug, Copy, Clone, PartialEq)]
enum PeerState {
    /// The peer has not yet been contacted.
    ///
    /// This is the starting state for every peer.
    NotContacted,

    /// The query has been started, waiting for a result from the peer.
    Waiting,

    /// A result was not delivered for the peer within the configured timeout.
    Unreachable,

    /// A successful result from the peer has been delivered.
    Succeeded,
}

impl PeerWithState {
    fn new(peer: KadPeer) -> Self {
        Self {
            peer,
            state: PeerState::NotContacted,
        }
    }
}

pub(crate) enum QueryUpdate {
    Queried {
        source: PeerId,
        closer: Vec<KadPeer>,
        // For GetProvider.
        provider: Option<Vec<KadPeer>>,
        // For GetValue.
        record: Option<record::Record>,
        // How long this query job takes.
        duration: Duration,
    },
    Unreachable(PeerId),
}

/// A record either received by the given peer or retrieved from the local
/// record store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecord {
    /// The peer from whom the record was received. `None` if the record was
    /// retrieved from local storage.
    pub peer: Option<PeerId>,
    pub record: record::Record,
}

/// The query result returned by IterativeQuery.
pub(crate) struct QueryResult {
    pub(crate) closest_peers: Option<Vec<KadPeer>>,
    pub(crate) found_peer: Option<KadPeer>,
    pub(crate) providers: Option<Vec<KadPeer>>,
    pub(crate) records: Option<Vec<PeerRecord>>,
    // the closest peers which don't have the value we are querying
    pub(crate) cache_peers: Option<Vec<PeerId>>,
}

pub(crate) struct ClosestPeers {
    /// The target key.
    target: Key<record::Key>,
    /// All queried peers sorted by distance.
    closest_peers: BTreeMap<Distance, PeerWithState>,
}

impl ClosestPeers {
    fn new(key: record::Key) -> Self {
        Self {
            target: Key::new(key),
            closest_peers: BTreeMap::new(),
        }
    }

    // helper of calculating key and distance
    fn distance(&self, peer_id: PeerId) -> Distance {
        let rkey = record::Key::from(peer_id);
        let key = Key::new(rkey);
        key.distance(&self.target)
    }

    // extend the btree with an vector of peer_id
    fn add_peers(&mut self, peers: Vec<KadPeer>) {
        // Incorporate the reported peers into the btree map.
        for peer in peers {
            let distance = self.distance(peer.node_id);
            self.closest_peers.entry(distance).or_insert_with(|| PeerWithState::new(peer));
        }
    }

    // check if the map contains the target already
    fn has_target(&self) -> Option<KadPeer> {
        // distance must be 0
        let distance = self.target.distance(&self.target);
        self.closest_peers.get(&distance).map(|p| p.peer.clone())
    }

    fn peers_filter(&self, num: usize, p: impl FnMut(&&PeerWithState) -> bool) -> impl Iterator<Item=&PeerWithState> {
        self.closest_peers.values().filter(p).take(num)
    }

    fn peers_in_state_mut(&mut self, state: PeerState, num: usize) -> impl Iterator<Item=&mut PeerWithState> {
        self.closest_peers.values_mut().filter(move |peer| peer.state == state).take(num)
    }

    fn peers_in_states(&self, states: Vec<PeerState>, num: usize) -> impl Iterator<Item=&PeerWithState> {
        self.closest_peers
            .values()
            .filter(move |peer| states.contains(&peer.state))
            .take(num)
    }

    fn get_peer_state(&self, peer_id: &PeerId) -> Option<PeerState> {
        let distance = self.distance(*peer_id);
        self.closest_peers.get(&distance).map(|peer| peer.state)
    }

    fn set_peer_state(&mut self, peer_id: &PeerId, state: PeerState) {
        let distance = self.distance(*peer_id);
        if let Some(peer) = self.closest_peers.get_mut(&distance) {
            peer.state = state
        }
    }

    fn num_of_state(&self, state: PeerState) -> usize {
        self.closest_peers.values().filter(|p| p.state == state).count()
    }

    fn num_of_states(&self, states: Vec<PeerState>) -> usize {
        self.closest_peers.values().filter(|p| states.contains(&p.state)).count()
    }

    fn is_starved(&self) -> bool {
        // starvation, if no peer to contact and no pending query
        self.num_of_states(vec![PeerState::NotContacted, PeerState::Waiting]) == 0
    }

    fn can_terminate(&self, beta_value: usize) -> bool {
        let closest = self
            .closest_peers
            .values()
            .filter(|peer| {
                peer.state == PeerState::NotContacted || peer.state == PeerState::Waiting || peer.state == PeerState::Succeeded
            })
            .take(beta_value);

        for peer in closest {
            if peer.state != PeerState::Succeeded {
                return false;
            }
        }
        true
    }
}

/// Information about a running query.
#[derive(Debug, Clone)]
pub enum QueryType {
    /// A query initiated by [`Kademlia::find_peer`].
    FindPeer,
    /// A query initiated by [`Kademlia::get_closest_peers`].
    GetClosestPeers,
    /// A query initiated by [`Kademlia::get_providers`].
    GetProviders {
        /// How many providers is needed before completing.
        count: usize,
        /// Providers found locally.
        local: Option<Vec<KadPeer>>,
    },
    /// A query initiated by [`Kademlia::get_record`].
    GetRecord {
        /// The quorum needed by get_record.
        quorum: usize,
        /// Records found locally.
        local: Option<Vec<PeerRecord>>,
    },
    AddProvider {
        provider: record::ProviderRecord,
        addresses: Vec<Multiaddr>,
    },
    PutRecord {
        record: record::Record,
    },
}

/// The iterative query.
///
/// It runs a iterative lookup for FindNode, GetProviders and GetValue operations, starting
/// with the seed peers. The output of every query will be collected to a sorted b-tree for
/// determining the closer peers, which might be engaged in the next round query.
///
/// The iterative operation could be terminated if:
/// 1. No more peer to lookup - starvation
/// 2. No closer peer found event after 'beta' round queries.
/// 3. Got the node/provider/value for the respective operation
pub(crate) struct IterativeQuery {
    /// The target to be queried.
    key: record::Key,
    /// The Swarm controller is used to communicate with Swarm.
    swarm: SwarmControl,
    /// The Messenger is used to send/receive Kad messages.
    messengers: MessengerManager,
    /// The query type to be executed.
    query_type: QueryType,
    /// The local peer Id of myself.
    local_id: PeerId,
    /// The kad configurations for queries.
    config: QueryConfig,
    /// The seed peers used to start the query.
    seeds: Vec<Key<PeerId>>,
    /// The KadPoster used to post ProtocolEvent to Kad main loop.
    poster: KadPoster,
    /// The statistics.
    stats: Arc<QueryStatsAtomic>,
    /// The ledger of all peer.
    ledgers: Arc<MetricMap<PeerId, Ledger>>,
}

impl IterativeQuery {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        query_type: QueryType,
        key: record::Key,
        swarm: SwarmControl,
        messengers: MessengerManager,
        local_id: PeerId,
        config: QueryConfig,
        seeds: Vec<Key<PeerId>>,
        poster: KadPoster,
        stats: Arc<QueryStatsAtomic>,
        ledgers: Arc<MetricMap<PeerId, Ledger>>,
    ) -> Self {
        Self {
            query_type,
            key,
            swarm,
            messengers,
            local_id,
            config,
            seeds,
            poster,
            stats,
            ledgers,
        }
    }

    pub(crate) async fn handle_update(
        &mut self,
        update: QueryUpdate,
        query_results: &mut QueryResult,
        closest_peers: &mut ClosestPeers,
    ) -> bool {
        let me = self;
        let k_value = me.config.k_value.get();

        // handle update, update the closest_peers, and update the kbuckets for new peer
        // or dead peer detected, update the QueryResult
        // TODO: check the state before setting state??
        match update {
            QueryUpdate::Queried {
                source,
                mut closer,
                provider,
                record,
                duration,
            } => {
                // incorporate 'closer' into 'clostest_peers', marked as PeerState::NotContacted
                log::debug!(
                    "successful query from {:}, found {} closer peers. {:?}",
                    source,
                    closer.len(),
                    duration
                );
                log::trace!("{:?} returns closer peers: {:?}", source, closer);

                // note we don't add myself
                closer.retain(|p| p.node_id != me.local_id);

                // update the PeerStore for the multiaddr, add all multiaddr of Closer peers
                // to PeerStore
                for peer in closer.iter() {
                    // kingwel, we filter out all loopback addresses
                    let addrs = peer.clone().multiaddrs;
                    me.swarm.add_addrs(&peer.node_id, addrs, TEMP_ADDR_TTL);
                }

                closest_peers.add_peers(closer);
                closest_peers.set_peer_state(&source, PeerState::Succeeded);

                // signal the k-buckets for new peer found
                let _ = me.poster.post(ProtocolEvent::KadPeerFound(source, true)).await;

                // handle different query type, check if we are done querying
                match me.query_type {
                    QueryType::GetClosestPeers => {}
                    QueryType::FindPeer => {
                        if let Some(peer) = closest_peers.has_target() {
                            log::debug!("FindPeer: successfully located, {:?}", peer);
                            query_results.found_peer = Some(peer);
                            return true;
                        }
                    }
                    QueryType::GetProviders { count, .. } => {
                        // update providers
                        if let Some(provider) = provider {
                            log::debug!("GetProviders: provider found {:?} key={:?}", provider, me.key);

                            // update the PeerStore for the multiaddr, add all multiaddr of Closer peers
                            // to PeerStore
                            for peer in provider.iter() {
                                me.swarm.add_addrs(&peer.node_id, peer.multiaddrs.clone(), PROVIDER_ADDR_TTL);
                            }

                            if !provider.is_empty() {
                                // append or create the query_results.providers
                                if let Some(mut old) = query_results.providers.take() {
                                    old.extend(provider);
                                    // remove duplicated peers
                                    old.sort_unstable_by(|a, b| a.node_id.partial_cmp(&b.node_id).unwrap());
                                    old.dedup_by(|a, b| a.node_id == b.node_id);
                                    query_results.providers = Some(old);
                                } else {
                                    query_results.providers = Some(provider);
                                }
                                // check if we have enough providers
                                if count != 0 && query_results.providers.as_ref().map_or(0, |p| p.len()) >= count {
                                    log::debug!("GetProviders: got enough provider for {:?}, limit={}", me.key, count);
                                    return true;
                                }
                            }
                        }
                    }
                    QueryType::GetRecord { quorum, .. } => {
                        if let Some(record) = record {
                            log::debug!("GetRecord: record found {:?} key={:?}", record, me.key);

                            let pr = PeerRecord {
                                peer: Some(source),
                                record,
                            };
                            if let Some(mut old) = query_results.records.take() {
                                old.push(pr);
                                query_results.records = Some(old);
                            } else {
                                query_results.records = Some(vec![pr]);
                            }

                            // check if we have enough records
                            if query_results.records.as_ref().map_or(0, |r| r.len()) > quorum {
                                log::info!("GetRecord: got enough records for key={:?}", me.key);

                                let peers_have_value = query_results
                                    .records
                                    .as_ref()
                                    .expect("must be Some")
                                    .iter()
                                    .filter_map(|r| r.peer.as_ref())
                                    .collect::<Vec<_>>();

                                // figure out the closest peers which didn't have the record
                                let peers = closest_peers
                                    .peers_filter(k_value, |p| {
                                        p.state != PeerState::Unreachable && !peers_have_value.contains(&&p.peer.node_id)
                                    })
                                    .map(|p| p.peer.node_id)
                                    .collect::<Vec<_>>();

                                log::debug!("GetValue, got peers which don't have the value, {:?}", peers);
                                query_results.cache_peers = Some(peers);
                                return true;
                            }
                        }
                    }
                    _ => panic!("not gonna happen"),
                }
            }
            QueryUpdate::Unreachable(peer) => {
                // set to PeerState::Unreachable
                log::debug!("unreachable peer {:?} detected", peer);

                closest_peers.set_peer_state(&peer, PeerState::Unreachable);
                // signal for dead peer detected
                let _ = me.poster.post(ProtocolEvent::KadPeerStopped(peer)).await;
            }
        }

        false
    }

    pub(crate) fn run<F>(self, f: F)
        where
            F: FnOnce(Result<QueryResult>) + Send + 'static,
    {
        log::debug!("run iterative query {:?} for {:?}", self.query_type, self.key);

        // check for empty seed peers
        if self.seeds.is_empty() {
            log::info!("no seeds, abort running");
            f(Err(KadError::NoKnownPeers));
            return;
        }

        // update stats
        let index = self.stats.iter_query_executed.fetch_add(1, Ordering::SeqCst);

        let mut me = self;
        let alpha_value = me.config.alpha_value.get();
        let beta_value = me.config.beta_value.get();
        let k_value = me.config.k_value.get();
        let timeout = me.config.timeout;
        let start = Instant::now();

        // closest_peers is used to retrieve the closer peers. It is a sorted btree-map, which is
        // indexed by Distance of the peer. The queried 'key' is used to calculate the distance.
        let mut closest_peers = ClosestPeers::new(me.key.clone());
        // prepare the query result
        let mut query_results = QueryResult {
            closest_peers: None,
            found_peer: None,
            providers: None,
            records: None,
            cache_peers: None,
        };

        // extract local PeerRecord or Providers from QueryType
        // local items are parts of the final result
        match &mut me.query_type {
            QueryType::GetProviders { count: _, local } => {
                query_results.providers = local.take();
            }
            QueryType::GetRecord { quorum: _, local } => {
                query_results.records = local.take();
            }
            _ => {}
        }

        // the channel used to deliver the result of each jobs
        let (mut tx, mut rx) = mpsc::channel(alpha_value);

        // statistics for this iterative query
        // This stat has to be referred by thwo futures: query & deadline, so it is implemented as
        // an Arc<Atomic...>
        let stats = me.stats.clone();
        // deadline for an iterative query
        let deadline = async move {
            task::sleep(timeout).await;
            stats.iter_query_timeout.fetch_add(1, Ordering::SeqCst);
            log::info!("iterative query timeout");
        };

        // clone stats and move into query
        let stats = me.stats.clone();
        // a runtime for query
        let query = async move {
            let seeds = me
                .seeds
                .iter()
                .map(|k| {
                    let id = k.clone().into_preimage();
                    KadPeer {
                        node_id: id,
                        multiaddrs: me.swarm.get_addrs(&id).unwrap_or_default(),
                        connection_ty: KadConnectionType::CanConnect,
                    }
                })
                .collect();

            // deliver the seeds to kick off the initial query
            let _ = tx
                .send(QueryUpdate::Queried {
                    source: me.local_id,
                    closer: seeds,
                    provider: None,
                    record: None,
                    duration: Duration::from_secs(0),
                })
                .await;

            // starting iterative querying for all selected peers...
            loop {
                // note that the first update comes from the initial seeds
                let update = rx.next().await.expect("must");
                let is_completed = me.handle_update(update, &mut query_results, &mut closest_peers).await;
                if is_completed {
                    log::debug!("iterative query completed due to value found");
                    break;
                }

                // starvation, if no peer to contact and no pending query
                if closest_peers.is_starved() {
                    //return true, LookupStarvation, nil
                    log::debug!("iterative query terminated due to starvation(no peer to contact and no pending query)");
                    break;
                }
                // meet the k_value? meaning lookup completed
                if closest_peers.can_terminate(beta_value) {
                    //return true, LookupCompleted, nil
                    log::debug!("iterative query terminated due to no more closer peer");
                    break;
                }

                // calculate the maximum number of queries we could be running
                // Note: NumWaiting will be updated before invoking job.execute()
                let num_jobs = alpha_value.checked_sub(closest_peers.num_of_state(PeerState::Waiting)).unwrap();

                log::debug!("iterative query, starting {} query jobs at most", num_jobs);

                let peer_iter = closest_peers.peers_in_state_mut(PeerState::NotContacted, num_jobs);
                for peer in peer_iter {
                    //closest_peers.set_peer_state(&peer, PeerState::Waiting);
                    peer.state = PeerState::Waiting;
                    let peer_id = peer.peer.node_id;

                    log::debug!("creating query job for {:?}", peer_id);

                    stats.iterative.requests.fetch_add(1, Ordering::SeqCst);

                    let job = QueryJob {
                        key: me.key.clone(),
                        qt: me.query_type.clone(),
                        messengers: me.messengers.clone(),
                        peer: peer_id,
                        addrs: me.swarm.get_addrs(&peer_id).unwrap_or_default(),
                        stats: stats.clone(),
                        ledgers: me.ledgers.clone(),
                        tx: tx.clone(),
                    };

                    let mut tx = tx.clone();
                    let _ = task::spawn(async move {
                        let stats = job.stats.clone();
                        let ledgers = job.ledgers.clone();
                        let pid = job.peer;
                        let addrs = job.addrs.clone();
                        let start = Instant::now();
                        let r = job.execute().await;
                        let cost = start.elapsed();
                        if r.is_err() {
                            let public_ips: Vec<Multiaddr> = addrs.into_iter().filter(|addr| !addr.is_private_addr() && !addr.is_ipv6_addr()).collect();
                            log::error!("index {}， cost {:?}, failed to talk to {}, all private ip {} err={:?}", index, cost, pid, public_ips.is_empty(), r);
                            stats.iterative.failure.fetch_add(1, Ordering::SeqCst);
                            let addition = Ledger {
                                succeed: Arc::new(AtomicU32::new(0)),
                                succeed_cost: Default::default(),
                                failed: Arc::new(AtomicU32::new(1)),
                                failed_cost: Arc::new(AtomicU64::new(cost.as_millis() as u64)),
                            };
                            ledgers.store_or_modify(&pid, addition.clone(), |_, ledger| { let _ = ledger.add(addition.clone()); });
                            let _ = tx.send(QueryUpdate::Unreachable(peer_id)).await;
                        } else {
                            // log::info!("index {}， cost {:?}, succeed to talk to {}", index, cost, pid);
                            let addition = Ledger {
                                succeed: Arc::new(AtomicU32::new(1)),
                                succeed_cost: Arc::new(AtomicU64::new(cost.as_millis() as u64)),
                                failed: Arc::new(AtomicU32::new(0)),
                                failed_cost: Default::default(),
                            };
                            ledgers.store_or_modify(&pid, addition.clone(), |_, ledger| { let _ = ledger.add(addition.clone()); });
                            stats.iterative.success.fetch_add(1, Ordering::SeqCst);
                        }
                    });
                }
            }

            // collect the query result
            let wanted_states = vec![PeerState::NotContacted, PeerState::Waiting, PeerState::Succeeded];
            let peers = closest_peers
                .peers_in_states(wanted_states, k_value)
                .map(|p| p.peer.clone())
                .collect::<Vec<_>>();

            log::debug!("iterative query, return {} closer peers", peers.len());
            log::debug!(
                "Closest Peers in details: Unreachable:{} NotContacted:{} Waiting:{} Succeeded:{}",
                closest_peers.num_of_state(PeerState::Unreachable),
                closest_peers.num_of_state(PeerState::NotContacted),
                closest_peers.num_of_state(PeerState::Waiting),
                closest_peers.num_of_state(PeerState::Succeeded)
            );

            if !peers.is_empty() {
                query_results.closest_peers = Some(peers);
            }

            stats.iter_query_completed.fetch_add(1, Ordering::SeqCst);

            // calculate how long this iterative query lasts
            let duration = start.elapsed();

            log::info!("index {}, iterative query report : {:?} {:?}", index, stats.iterative.to_view(), duration);

            Ok(query_results)
        };

        task::spawn(async {
            let either = futures::future::select(query.boxed(), deadline.boxed()).await;
            match either {
                Either::Left((result, _)) => f(result),
                Either::Right((_, _)) => f(Err(KadError::Timeout)),
            }
        });
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod test_closest_peers {
    use super::*;

    #[test]
    fn test_closest_peers_distance() {
        let peer = PeerId::random();
        let peer_cloned = peer;

        let mut closest_peers = ClosestPeers::new(peer.into());

        let kad_peer = KadPeer {
            node_id: peer_cloned,
            multiaddrs: vec![],
            connection_ty: KadConnectionType::NotConnected,
        };

        assert!(closest_peers.has_target().is_none());
        closest_peers.add_peers(vec![kad_peer]);
        assert!(closest_peers.has_target().is_some());
    }

    #[test]
    fn test_starved() {
        let peer = PeerId::random();

        let mut closest_peers = ClosestPeers::new(PeerId::random().into());
        assert!(closest_peers.is_starved());

        let kad_peer = KadPeer {
            node_id: peer,
            multiaddrs: vec![],
            connection_ty: KadConnectionType::NotConnected,
        };

        // There are only one peer in closest_peers.
        // The state of new added peer is NotContacted, should be not starved
        closest_peers.add_peers(vec![kad_peer]);
        assert!(!closest_peers.is_starved());

        let state = closest_peers.get_peer_state(&peer);
        assert_eq!(state, Some(PeerState::NotContacted));

        // The state of peer is NotContacted, should be starved
        closest_peers.set_peer_state(&peer, PeerState::Succeeded);
        let state = closest_peers.get_peer_state(&peer);
        assert_eq!(state, Some(PeerState::Succeeded));

        assert!(closest_peers.is_starved());
    }

    #[test]
    fn test_terminate() {
        // closest_peers is empty, can terminate
        let mut closest_peers = ClosestPeers::new(PeerId::random().into());
        assert!(closest_peers.can_terminate(1));

        let peer1 = PeerId::random();
        let kad_peer1 = KadPeer {
            node_id: peer1,
            multiaddrs: vec![],
            connection_ty: KadConnectionType::NotConnected,
        };

        // closest_peers has 1 peer and its state is NotContacted, can not terminate
        closest_peers.add_peers(vec![kad_peer1]);
        assert!(!closest_peers.can_terminate(1));

        // closest_peers has 1 peer and its state is Succeeded, can terminate
        closest_peers.set_peer_state(&peer1, PeerState::Succeeded);
        assert!(closest_peers.can_terminate(1));
    }
}

// #[cfg(test)]
// mod test_fixed_query {
//     use crate::query::{FixedQuery, QueryType};
//
//     #[test]
//     fn test_fixed_query() {
//         FixedQuery::new(QueryType::FindPeer, );
//     }
// }
