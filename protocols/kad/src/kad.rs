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

use fnv::{FnvHashMap, FnvHashSet};
use std::borrow::Borrow;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    select,
};

use async_std::task;
use async_std::task::JoinHandle;

use libp2prs_core::{Multiaddr, PeerId, ProtocolId};
use libp2prs_swarm::Control as SwarmControl;

use crate::control::{Control, ControlCommand, DumpCommand};
use crate::protocol::{
    KadConnectionType, KadMessenger, KadMessengerView, KadPeer, KadProtocolHandler, KadRequestMsg, KadResponseMsg,
    KademliaProtocolConfig, ProtocolEvent, RefreshStage,
};

use crate::addresses::PeerInfo;
use crate::kbucket::KBucketsTable;
use crate::query::{FixedQuery, IterativeQuery, PeerRecord, QueryConfig, QueryStats, QueryType};
use crate::store::RecordStore;
use crate::{kbucket, record, KadError, ProviderRecord, Record};
use libp2prs_core::peerstore::{ADDRESS_TTL, PROVIDER_ADDR_TTL};

type Result<T> = std::result::Result<T, KadError>;

/// `Kademlia` implements the libp2p Kademlia protocol.
pub struct Kademlia<TStore> {
    /// The Kademlia routing table.
    kbuckets: KBucketsTable<kbucket::Key<PeerId>, PeerInfo>,

    /// Configuration of the wire protocol.
    protocol_config: KademliaProtocolConfig,

    /// Flag to indicate if refreshing is ongoing.
    refreshing: bool,

    /// The config for queries.
    query_config: QueryConfig,

    /// The statistics of Kad DHT.
    stats: KademliaStats,

    /// The cache manager of Kademlia messenger.
    messengers: Option<MessengerManager>,

    /// The currently connected peers.
    ///
    /// This is a superset of the connected peers currently in the routing table.
    connected_peers: FnvHashSet<PeerId>,

    /// The timer task handle of Provider cleanup job.
    provider_timer_handle: Option<JoinHandle<()>>,

    /// The timer task handle of Provider cleanup job.
    refresh_timer_handle: Option<JoinHandle<()>>,

    /// The periodic interval to cleanup expired provider records.
    cleanup_interval: Duration,

    /// The periodic interval to refreshing the routing table.
    /// `None` disables auto refresh.
    refresh_interval: Option<Duration>,

    /// The TTL of regular (value-)records.
    record_ttl: Option<Duration>,

    /// The TTL of provider records.
    provider_record_ttl: Option<Duration>,

    /// How long to keep connections alive when they're idle.
    connection_idle_timeout: Duration,

    /// How long to ping/check a Kad peer since last time we talk to them.
    check_kad_peer_interval: Duration,

    /// The currently known addresses of the local node.
    ///
    /// The addresses come from Swarm when initializing Kademlia.
    local_addrs: Vec<Multiaddr>,

    /// The record storage.
    store: TStore,

    // Used to communicate with Swarm.
    swarm: Option<SwarmControl>,

    // New peer is connected or peer is dead.
    // peer_tx: mpsc::UnboundedSender<PeerEvent>,
    // peer_rx: mpsc::UnboundedReceiver<PeerEvent>,
    /// Used to handle the incoming Kad events.
    event_tx: mpsc::UnboundedSender<ProtocolEvent>,
    event_rx: mpsc::UnboundedReceiver<ProtocolEvent>,

    /// Used to control the Kademlia.
    /// control_tx becomes the Control and control_rx is monitored by
    /// the Kademlia main loop.
    control_tx: mpsc::UnboundedSender<ControlCommand>,
    control_rx: mpsc::UnboundedReceiver<ControlCommand>,
}

/// The statistics of Kademlia.
#[derive(Debug, Clone, Default)]
pub struct KademliaStats {
    pub successful_queries: usize,
    pub timeout_queries: usize,
    pub total_refreshes: usize,
    pub query: QueryStats,
    pub message_rx: MessageStats,
}

#[derive(Debug, Clone, Default)]
pub struct MessageStats {
    pub(crate) ping: usize,
    pub(crate) find_node: usize,
    pub(crate) get_provider: usize,
    pub(crate) add_provider: usize,
    pub(crate) get_value: usize,
    pub(crate) put_value: usize,
}

/// The configuration for the `Kademlia` behaviour.
///
/// The configuration is consumed by [`Kademlia::new`].
#[derive(Debug, Clone)]
pub struct KademliaConfig {
    query_config: QueryConfig,
    protocol_config: KademliaProtocolConfig,
    refresh_interval: Option<Duration>,
    cleanup_interval: Duration,
    record_ttl: Option<Duration>,
    record_replication_interval: Option<Duration>,
    record_publication_interval: Option<Duration>,
    provider_record_ttl: Option<Duration>,
    provider_publication_interval: Option<Duration>,
    connection_idle_timeout: Duration,
    check_kad_peer_interval: Duration,
}

impl Default for KademliaConfig {
    fn default() -> Self {
        let check_kad_peer_interval = Duration::from_secs(10 * 60);
        // TODO: calculate check_kad_peer_interval according to go-libp2p
        // // The threshold is calculated based on the expected amount of time that should pass before we
        // // query a peer as part of our refresh cycle.
        // // To grok the Math Wizardy that produced these exact equations, please be patient as a document explaining it will
        // // be published soon.
        // if cfg.concurrency < cfg.bucketSize { // (alpha < K)
        //     l1 := math.Log(float64(1) / float64(cfg.bucketSize))                              //(Log(1/K))
        //     l2 := math.Log(float64(1) - (float64(cfg.concurrency) / float64(cfg.bucketSize))) // Log(1 - (alpha / K))
        //     maxLastSuccessfulOutboundThreshold = time.Duration(l1 / l2 * float64(cfg.routingTable.refreshInterval))
        // } else {
        //     maxLastSuccessfulOutboundThreshold = cfg.routingTable.refreshInterval
        // }
        KademliaConfig {
            query_config: QueryConfig::default(),
            protocol_config: Default::default(),
            refresh_interval: Some(Duration::from_secs(10 * 60)),
            cleanup_interval: Duration::from_secs(24 * 60 * 60),
            record_ttl: Some(Duration::from_secs(36 * 60 * 60)),
            record_replication_interval: Some(Duration::from_secs(60 * 60)),
            record_publication_interval: Some(Duration::from_secs(24 * 60 * 60)),
            provider_publication_interval: Some(Duration::from_secs(12 * 60 * 60)),
            provider_record_ttl: Some(Duration::from_secs(24 * 60 * 60)),
            connection_idle_timeout: Duration::from_secs(10),
            check_kad_peer_interval,
        }
    }
}

impl KademliaConfig {
    /// Sets a custom protocol name.
    ///
    /// Kademlia nodes only communicate with other nodes using the same protocol
    /// name. Using a custom name therefore allows to segregate the DHT from
    /// others, if that is desired.
    pub fn with_protocol_name(mut self, name: ProtocolId) -> Self {
        self.protocol_config.set_protocol_name(name);
        self
    }

    /// Sets the timeout for a single query.
    ///
    /// > **Note**: A single query usually comprises at least as many requests
    /// > as the replication factor, i.e. this is not a request timeout.
    ///
    /// The default is 60 seconds.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_config.timeout = timeout;
        self
    }

    /// Sets the K value to use. The default is [`K_VALUE`].
    pub fn with_k(mut self, k: NonZeroUsize) -> Self {
        self.query_config.k_value = k;
        self
    }

    /// Sets the allowed level of parallelism for iterative queries.
    ///
    /// The `α` parameter in the Kademlia paper. The maximum number of peers
    /// that an iterative query is allowed to wait for in parallel while
    /// iterating towards the closest nodes to a target. Defaults to
    /// `ALPHA_VALUE`.
    pub fn with_alpha(mut self, alpha: NonZeroUsize) -> Self {
        self.query_config.alpha_value = alpha;
        self
    }

    /// Sets the beta value for iterative queries.
    ///
    /// The number of peers closest to a target that must have responded
    /// for an iterative query to terminate. Defaults to `BETA_VALUE`.
    pub fn with_beta(mut self, beta: NonZeroUsize) -> Self {
        self.query_config.beta_value = beta;
        self
    }

    /// Sets the interval for routing table refresh.
    ///
    /// The Kad routing table will be refreshed automatically per the interval.
    /// The default is 10 minutes. Sets to `None` to disable auto refresh.
    ///
    pub fn with_refresh_interval(mut self, interval: Option<Duration>) -> Self {
        self.refresh_interval = interval;
        self
    }

    /// Sets the interval for Provider cleanup.
    ///
    /// The provider records will be cleaned up per the interval.The default is 1
    /// hour.
    ///
    pub fn with_cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Sets the TTL for stored records.
    ///
    /// The TTL should be significantly longer than the (re-)publication
    /// interval, to avoid premature expiration of records. The default is 36
    /// hours.
    ///
    /// `None` means records never expire.
    ///
    /// Does not apply to provider records.
    pub fn with_record_ttl(mut self, record_ttl: Option<Duration>) -> Self {
        self.record_ttl = record_ttl;
        self
    }

    /// Sets the (re-)replication interval for stored records.
    ///
    /// Periodic replication of stored records ensures that the records
    /// are always replicated to the available nodes closest to the key in the
    /// context of DHT topology changes (i.e. nodes joining and leaving), thus
    /// ensuring persistence until the record expires. Replication does not
    /// prolong the regular lifetime of a record (for otherwise it would live
    /// forever regardless of the configured TTL). The expiry of a record
    /// is only extended through re-publication.
    ///
    /// This interval should be significantly shorter than the publication
    /// interval, to ensure persistence between re-publications. The default
    /// is 1 hour.
    ///
    /// `None` means that stored records are never re-replicated.
    ///
    /// Does not apply to provider records.
    pub fn with_replication_interval(mut self, interval: Option<Duration>) -> Self {
        self.record_replication_interval = interval;
        self
    }

    /// Sets the (re-)publication interval of stored records.
    ///
    /// Records persist in the DHT until they expire. By default, published
    /// records are re-published in regular intervals for as long as the record
    /// exists in the local storage of the original publisher, thereby extending
    /// the records lifetime.
    ///
    /// This interval should be significantly shorter than the record TTL, to
    /// ensure records do not expire prematurely. The default is 24 hours.
    ///
    /// `None` means that stored records are never automatically re-published.
    ///
    /// Does not apply to provider records.
    pub fn with_publication_interval(mut self, interval: Option<Duration>) -> Self {
        self.record_publication_interval = interval;
        self
    }

    /// Sets the TTL for provider records.
    ///
    /// `None` means that stored provider records never expire.
    ///
    /// Must be significantly larger than the provider publication interval.
    pub fn with_provider_record_ttl(mut self, ttl: Option<Duration>) -> Self {
        self.provider_record_ttl = ttl;
        self
    }

    /// Sets the interval at which provider records for keys provided
    /// by the local node are re-published.
    ///
    /// `None` means that stored provider records are never automatically
    /// re-published.
    ///
    /// Must be significantly less than the provider record TTL.
    pub fn with_provider_publication_interval(mut self, interval: Option<Duration>) -> Self {
        self.provider_publication_interval = interval;
        self
    }

    /// Sets the amount of time to keep connections alive when they're idle.
    pub fn with_connection_idle_timeout(mut self, duration: Duration) -> Self {
        self.connection_idle_timeout = duration;
        self
    }

    /// Modifies the maximum allowed size of individual Kademlia packets.
    ///
    /// It might be necessary to increase this value if trying to put large
    /// records.
    pub fn with_max_packet_size(mut self, size: usize) -> Self {
        self.protocol_config.set_max_packet_size(size);
        self
    }
}

/// KadPoster is used to generate ProtocolEvent to Kad main loop.
#[derive(Clone, Debug)]
pub(crate) struct KadPoster(mpsc::UnboundedSender<ProtocolEvent>);

impl KadPoster {
    pub(crate) fn new(tx: mpsc::UnboundedSender<ProtocolEvent>) -> Self {
        Self(tx)
    }

    pub(crate) async fn post(&mut self, event: ProtocolEvent) -> Result<()> {
        self.0.send(event).await?;
        Ok(())
    }
}

impl<TStore> Kademlia<TStore>
where
    for<'a> TStore: RecordStore<'a> + Send + 'static,
{
    /// Creates a new `Kademlia` network behaviour with a default configuration.
    pub fn new(id: PeerId, store: TStore) -> Self {
        Self::with_config(id, store, Default::default())
    }

    // /// Get the protocol name of this kademlia instance.
    // pub fn protocol_name(&self) -> &[u8] {
    //     self.protocol_config.protocol_name()
    // }

    /// Creates a new `Kademlia` network behaviour with the given configuration.
    pub fn with_config(id: PeerId, store: TStore, config: KademliaConfig) -> Self {
        let local_key = kbucket::Key::new(id);

        let (event_tx, event_rx) = mpsc::unbounded();
        let (control_tx, control_rx) = mpsc::unbounded();

        Kademlia {
            store,
            swarm: None,
            event_rx,
            event_tx,
            control_tx,
            control_rx,
            kbuckets: KBucketsTable::new(local_key),
            protocol_config: config.protocol_config,
            refreshing: false,
            query_config: config.query_config,
            stats: Default::default(),
            messengers: None,
            connected_peers: Default::default(),
            provider_timer_handle: None,
            refresh_timer_handle: None,
            cleanup_interval: config.cleanup_interval,
            refresh_interval: config.refresh_interval,
            record_ttl: config.record_ttl,
            provider_record_ttl: config.provider_record_ttl,
            connection_idle_timeout: config.connection_idle_timeout,
            check_kad_peer_interval: config.check_kad_peer_interval,
            local_addrs: vec![],
        }
    }

    // Returns a copied instance of Kad poster.
    fn poster(&self) -> KadPoster {
        KadPoster::new(self.event_tx.clone())
    }

    /*
        /// Adds a known listen address of a peer participating in the DHT to the
        /// routing table.
        ///
        /// Explicitly adding addresses of peers serves two purposes:
        ///
        ///   1. In order for a node to join the DHT, it must know about at least
        ///      one other node of the DHT.
        ///
        ///   2. When a remote peer initiates a connection and that peer is not
        ///      yet in the routing table, the `Kademlia` behaviour must be
        ///      informed of an address on which that peer is listening for
        ///      connections before it can be added to the routing table
        ///      from where it can subsequently be discovered by all peers
        ///      in the DHT.
        ///
        /// If the routing table has been updated as a result of this operation,
        /// a [`KademliaEvent::RoutingUpdated`] event is emitted.
        pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) -> RoutingUpdate {
            let key = kbucket::Key::new(peer.clone());
            match self.kbuckets.entry(&key) {
                kbucket::Entry::Present(mut entry, _) => {
                    if entry.value().insert(address) {
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                            KademliaEvent::RoutingUpdated {
                                peer: peer.clone(),
                                addresses: entry.value().clone(),
                                old_peer: None,
                            }
                        ))
                    }
                    RoutingUpdate::Success
                }
                kbucket::Entry::Pending(mut entry, _) => {
                    entry.value().insert(address);
                    RoutingUpdate::Pending
                }
                kbucket::Entry::Absent(entry) => {
                    let addresses = Addresses::new(address);
                    let status =
                        if self.connected_peers.contains(peer) {
                            NodeStatus::Connected
                        } else {
                            NodeStatus::Disconnected
                        };
                    match entry.insert(addresses.clone(), status) {
                        kbucket::InsertResult::Inserted => {
                            self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                                KademliaEvent::RoutingUpdated {
                                    peer: peer.clone(),
                                    addresses,
                                    old_peer: None,
                                }
                            ));
                            RoutingUpdate::Success
                        },
                        kbucket::InsertResult::Full => {
                            log::debug!("Bucket full. Peer not added to routing table: {}", peer);
                            RoutingUpdate::Failed
                        },
                        kbucket::InsertResult::Pending { disconnected } => {
                            self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                                peer_id: disconnected.into_preimage(),
                                condition: DialPeerCondition::Disconnected
                            });
                            RoutingUpdate::Pending
                        },
                    }
                },
                kbucket::Entry::SelfEntry => RoutingUpdate::Failed,
            }
        }

        /// Removes an address of a peer from the routing table.
        ///
        /// If the given address is the last address of the peer in the
        /// routing table, the peer is removed from the routing table
        /// and `Some` is returned with a view of the removed entry.
        /// The same applies if the peer is currently pending insertion
        /// into the routing table.
        ///
        /// If the given peer or address is not in the routing table,
        /// this is a no-op.
        pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr)
                              -> Option<kbucket::EntryView<kbucket::Key<PeerId>, Addresses>>
        {
            let key = kbucket::Key::new(peer.clone());
            match self.kbuckets.entry(&key) {
                kbucket::Entry::Present(mut entry, _) => {
                    if entry.value().remove(address).is_err() {
                        Some(entry.remove()) // it is the last address, thus remove the peer.
                    } else {
                        None
                    }
                }
                kbucket::Entry::Pending(mut entry, _) => {
                    if entry.value().remove(address).is_err() {
                        Some(entry.remove()) // it is the last address, thus remove the peer.
                    } else {
                        None
                    }
                }
                kbucket::Entry::Absent(..) | kbucket::Entry::SelfEntry => {
                    None
                }
            }
        }
    */
    /// Tries to add a peer into the routing table.
    ///
    /// 1. the peer is already in RT:
    ///    1.1. if the peer is that either we queried it or it queried us, we update the aliveness
    ///         to the current instant.
    ///    1.2  do nothing
    /// 2. the peers is not in RT
    ///    2.1 if the bucket to which the peer belongs is full, we try to replace an existing peer
    ///        whose aliveness is older than the current time by the maximum allowed check_interval,
    ///        with the new peer.
    ///    2.2 if there is no such peer exists in that bucket, do nothing -> ignore adding peer
    fn try_add_peer(&mut self, peer: PeerId, queried: bool) {
        let timeout = self.check_kad_peer_interval;
        let now = Instant::now();
        let key = kbucket::Key::new(peer.clone());

        log::debug!(
            "trying to add a peer: {:?} bucket-index={:?}, query={}",
            peer,
            self.kbuckets.bucket_index(&key),
            queried
        );

        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry) => {
                // already in RT, update the node's aliveness if queried is true
                if queried {
                    entry.value().set_aliveness(Some(Instant::now()));
                    log::debug!("{:?} updated: {:?}", peer, entry.value());
                }
            }
            kbucket::Entry::Absent(mut entry) => {
                let info = PeerInfo::new(queried);
                if entry.insert(info.clone()) {
                    log::debug!("Peer added to routing table: {} {:?}", peer, info);
                    // pin this peer in PeerStore to prevent GC from recycling multiaddr
                    if let Some(s) = self.swarm.as_ref() {
                        s.pin(&peer)
                    }
                } else {
                    log::debug!("Bucket full, trying to replace an old node for {}", peer);
                    // try replacing an 'old' peer
                    let bucket = entry.bucket();
                    let candidate = bucket
                        .iter()
                        .filter(|n| n.value.get_aliveness().map_or(true, |a| now.duration_since(a) > timeout))
                        .min_by(|x, y| x.value.get_aliveness().cmp(&y.value.get_aliveness()));

                    if let Some(candidate) = candidate {
                        let key = candidate.key.clone();
                        let evicted = bucket.remove(&key);
                        log::debug!("Bucket full. Peer node added, {} replacing {:?}", peer, evicted);
                        // unpin the evicted peer
                        if let Some(s) = self.swarm.as_ref() {
                            s.unpin(key.preimage())
                        }
                        // now try to insert the value again
                        let _ = entry.insert(info);
                        // pin this peer in PeerStore to prevent GC from recycling multiaddr
                        if let Some(s) = self.swarm.as_ref() {
                            s.pin(&peer)
                        }
                    } else {
                        log::debug!("Bucket full, but can't find an replaced node, give up {}", peer);
                    }
                }
            }
            _ => {}
        }
    }

    /// Tries to remove a peer from the routing table.
    ///
    /// Returns `None` if the peer was not in the routing table,
    /// not even pending insertion.
    fn try_remove_peer(&mut self, peer: PeerId) -> Option<kbucket::EntryView<kbucket::Key<PeerId>, PeerInfo>> {
        let key = kbucket::Key::new(peer.clone());

        log::debug!(
            "trying to remove a peer: {:?} bucket-index={:?}",
            peer,
            self.kbuckets.bucket_index(&key)
        );

        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(entry) => {
                // unpin the peer, so GC can run normally
                if let Some(s) = self.swarm.as_ref() {
                    s.unpin(&peer)
                }
                Some(entry.remove())
            }
            kbucket::Entry::Absent(..) | kbucket::Entry::SelfEntry => None,
        }
    }

    /// Tries to deactivate a Node in the routing table.
    ///
    /// Instead of removing the node from DHT, we set its aliveness to `None`.
    /// Thus, it could be replaced if any new node is going to be added. The
    /// reason of this approach is we'd keep an inactive node as long as the
    /// bucket is not full.
    fn try_deactivate_peer(&mut self, peer: PeerId) {
        let key = kbucket::Key::new(peer.clone());

        log::debug!(
            "trying to deactivate a peer: {:?} bucket-index={:?}",
            peer,
            self.kbuckets.bucket_index(&key)
        );

        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry) => entry.value().set_aliveness(None),
            kbucket::Entry::Absent(..) | kbucket::Entry::SelfEntry => {}
        }
    }

    /// Returns an iterator over all non-empty buckets in the routing table.
    fn kbuckets(&mut self) -> impl Iterator<Item = kbucket::KBucketRef<'_, kbucket::Key<PeerId>, PeerInfo>> {
        self.kbuckets.iter().filter(|b| !b.is_empty())
    }

    /// Returns the k-bucket for the distance to the given key.
    ///
    /// Returns `None` if the given key refers to the local key.
    pub fn kbucket<K>(&mut self, key: K) -> Option<kbucket::KBucketRef<'_, kbucket::Key<PeerId>, PeerInfo>>
    where
        K: Borrow<[u8]> + Clone,
    {
        self.kbuckets.bucket(&kbucket::Key::new(key))
    }

    /// Gets a mutable reference to the record store.
    pub fn store_mut(&mut self) -> &mut TStore {
        &mut self.store
    }

    // prepare and generate a IterativeQuery for iterative query.
    fn prepare_iterative_query(&mut self, qt: QueryType, key: record::Key) -> IterativeQuery {
        let local_id = self.kbuckets.self_key().preimage().clone();
        let target = kbucket::Key::new(key.clone());
        let seeds = self
            .kbuckets
            .closest_keys(&target)
            .into_iter()
            .take(self.query_config.k_value.get())
            .collect();

        IterativeQuery::new(
            qt,
            key,
            self.swarm.clone().expect("must be Some"),
            self.messengers.clone().expect("must be Some"),
            local_id,
            self.query_config.clone(),
            seeds,
            self.poster(),
        )
    }

    /// Initiates an iterative lookup for the closest peers to the given key.
    fn get_closest_peers<F>(&mut self, key: record::Key, f: F)
    where
        F: FnOnce(Result<Vec<KadPeer>>) + Send + 'static,
    {
        let q = self.prepare_iterative_query(QueryType::GetClosestPeers, key);

        q.run(|r| {
            f(r.and_then(|r| r.closest_peers.ok_or(KadError::NotFound)));
        });
    }

    /// Performs an iterative lookup for the closest peers to the given key.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<Option<KadPeer>>).
    fn find_peer<F>(&mut self, key: record::Key, f: F)
    where
        F: FnOnce(Result<KadPeer>) + Send + 'static,
    {
        let q = self.prepare_iterative_query(QueryType::FindPeer, key);

        q.run(|r| {
            f(r.and_then(|r| r.found_peer.ok_or(KadError::NotFound)));
        });
    }

    /// Performs a lookup for providers of a value to the given key.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<Vec<KadPeer>>).
    fn get_providers<F>(&mut self, key: record::Key, count: usize, f: F)
    where
        F: FnOnce(Result<Vec<KadPeer>>) + Send + 'static,
    {
        let provider_peers = self.provider_peers(&key, None);

        if provider_peers.len() >= count {
            // ok, we have enough providers for this key, simply return
            f(Ok(provider_peers));
        } else {
            let remaining = count - provider_peers.len();
            let q = self.prepare_iterative_query(
                QueryType::GetProviders {
                    count: remaining,
                    local: Some(provider_peers),
                },
                key,
            );

            q.run(|r| {
                f(r.and_then(|r| r.providers.ok_or(KadError::NotFound)));
            });
        }
    }

    /// Performs a lookup of a record to the given key.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<Vec<PeerRecord>>).
    fn get_record<F>(&mut self, key: record::Key, f: F)
    where
        F: FnOnce(Result<PeerRecord>) + Send + 'static,
    {
        let quorum = self.query_config.k_value.get();
        let mut records = Vec::with_capacity(quorum);

        if let Some(record) = self.store.get(&key) {
            if record.is_expired(Instant::now()) {
                self.store.remove(&key)
            } else {
                records.push(PeerRecord {
                    peer: None,
                    record: record.into_owned(),
                });
            }
        }

        if records.len() >= quorum {
            // ok, we have enough, simply return first item
            let record = records.first().cloned().map_or(Err(KadError::NotFound), Ok);
            f(record);
        } else {
            let config = self.query_config.clone();
            let messengers = self.messengers.clone().expect("must be Some");

            let needed = quorum - records.len();
            let q = self.prepare_iterative_query(
                QueryType::GetRecord {
                    quorum_needed: needed,
                    local: Some(records),
                },
                key,
            );
            q.run(|r| {
                f(r.and_then(|r| {
                    let record = r.records.as_ref().map(|r| r.first().cloned());
                    if let Some(Some(record)) = record.clone() {
                        if let Some(cache_peers) = r.cache_peers {
                            let record = record.record;
                            let fixed_query = FixedQuery::new(QueryType::PutRecord { record }, messengers, config, cache_peers);
                            fixed_query.run(|_| {});
                        }
                    }

                    // only need 1 item, take the first item
                    // r.records.ok_or(KadError::NotFound)
                    record.map_or(Err(KadError::NotFound), |r| r.map_or(Err(KadError::NotFound), Ok))
                }));
            });
        }
    }

    /// Stores a record in the DHT.
    ///
    /// The record is always stored locally with the given expiration. If the record's
    /// expiration is `None`, the common case, it does not expire in local storage
    /// but is still replicated with the configured record TTL. To remove the record
    /// locally and stop it from being re-published in the DHT, see [`Kademlia::remove_record`].
    ///
    /// After the initial publication of the record, it is subject to (re-)replication
    /// and (re-)publication as per the configured intervals. Periodic (re-)publication
    /// does not update the record's expiration in local storage, thus a given record
    /// with an explicit expiration will always expire at that instant and until then
    /// is subject to regular (re-)replication and (re-)publication.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<()>).
    fn put_record<F>(&mut self, key: record::Key, value: Vec<u8>, f: F)
    where
        F: FnOnce(Result<()>) + Send + 'static,
    {
        // TODO: probably we should check if there is a old record with the same key?

        let mut record = Record {
            key,
            value,
            publisher: None,
            expires: None,
        };

        record.publisher = Some(self.kbuckets.self_key().preimage().clone());
        if let Err(e) = self.store.put(record.clone()) {
            f(Err(e));
            return;
        }
        record.expires = record.expires.or_else(|| self.record_ttl.map(|ttl| Instant::now() + ttl));

        let config = self.query_config.clone();
        let messengers = self.messengers.clone().expect("must be Some");
        // initiate the iterative lookup for closest peers, which can be used to publish the record
        self.get_closest_peers(record.key.clone(), move |peers| {
            if let Err(e) = peers {
                f(Err(e));
            } else {
                let peers = peers.unwrap().into_iter().map(KadPeer::into).collect::<Vec<_>>();
                let fixed_query = FixedQuery::new(QueryType::PutRecord { record }, messengers, config, peers);
                fixed_query.run(f);
            }
        });
    }

    /// Removes the record with the given key from _local_ storage,
    /// if the local node is the publisher of the record.
    ///
    /// Has no effect if a record for the given key is stored locally but
    /// the local node is not a publisher of the record.
    ///
    /// This is a _local_ operation. However, it also has the effect that
    /// the record will no longer be periodically re-published, allowing the
    /// record to eventually expire throughout the DHT.
    fn remove_record(&mut self, key: &record::Key) {
        if let Some(r) = self.store.get(key) {
            if r.publisher.as_ref() == Some(self.kbuckets.self_key().preimage()) {
                self.store.remove(key)
            }
        }
    }

    /// Bootstraps the local node to join the DHT.
    ///
    /// Bootstrapping is a multi-step operation that starts with a lookup of the local node's
    /// own ID in the DHT. This introduces the local node to the other nodes
    /// in the DHT and populates its routing table with the closest neighbours.
    ///
    /// Subsequently, all buckets farther from the bucket of the closest neighbour are
    /// refreshed by initiating an additional bootstrapping query for each such
    /// bucket with random keys.
    ///
    /// > **Note**: Bootstrapping requires at least one node of the DHT to be known.
    async fn bootstrap(&mut self) {
        if !self.refreshing {
            log::debug!("bootstrapping...");
            let mut poster = self.poster();
            let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::Start)).await;
        }
    }

    fn add_node(&mut self, peer: PeerId, addresses: Vec<Multiaddr>) {
        if let Some(s) = self.swarm.as_ref() {
            s.add_addrs(&peer, addresses, ADDRESS_TTL)
        }
        self.try_add_peer(peer, false);
    }

    fn remove_node(&mut self, peer: PeerId) {
        if let Some(s) = self.swarm.as_ref() {
            s.clear_addrs(&peer)
        }
        self.try_remove_peer(peer);
    }

    fn dump_messengers(&mut self) -> Vec<KadMessengerView> {
        self.messengers.as_ref().expect("must be Some").messengers()
    }

    fn dump_statistics(&mut self) -> KademliaStats {
        self.stats.clone()
    }

    fn dump_kbuckets(&mut self) -> Vec<KBucketView> {
        let swarm = self.swarm.as_ref().expect("must be Some");
        let connected = &self.connected_peers;

        let entries = self
            .kbuckets
            .iter()
            .filter(|k| !k.is_empty())
            .map(|k| {
                let index = k.index();
                let bucket = k
                    .iter()
                    .map(|n| {
                        let id = n.node.key.preimage().clone();
                        let aliveness = n.node.value.get_aliveness();
                        let connected = connected.contains(&id);
                        let addresses = swarm.get_addrs(&id).unwrap_or_else(Vec::new);
                        KNodeView {
                            id,
                            aliveness,
                            addresses,
                            connected,
                        }
                    })
                    .collect::<Vec<_>>();

                KBucketView { index, bucket }
            })
            .collect::<Vec<_>>();

        entries
    }

    /// Performs publishing as a provider of a value for the given key.
    ///
    /// This operation publishes a provider record with the given key and
    /// identity of the local node to the peers closest to the key, thus establishing
    /// the local node as a provider.
    ///
    /// The publication of the provider records is periodically repeated as per the
    /// configured interval, to renew the expiry and account for changes to the DHT
    /// topology. A provider record may be removed from local storage and
    /// thus no longer re-published by calling [`Kademlia::stop_providing`].
    ///
    /// In contrast to the standard Kademlia push-based model for content distribution
    /// implemented by [`Kademlia::put_record`], the provider API implements a
    /// pull-based model that may be used in addition or as an alternative.
    /// The means by which the actual value is obtained from a provider is out of scope
    /// of the libp2p Kademlia provider API.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<()>).
    fn start_providing<F>(&mut self, key: record::Key, f: F)
    where
        F: FnOnce(Result<()>) + Send + 'static,
    {
        let provider = ProviderRecord::new(key.clone(), self.kbuckets.self_key().preimage().clone(), None);
        if let Err(e) = self.store.add_provider(provider.clone()) {
            f(Err(e));
            return;
        }
        let config = self.query_config.clone();
        let messengers = self.messengers.clone().expect("must be Some");
        let addresses = self.local_addrs.clone();

        // initiate the iterative lookup for closest peers, which can be used to publish the record
        self.get_closest_peers(key, move |peers| {
            if let Err(e) = peers {
                f(Err(e));
            } else {
                let peers = peers.unwrap().into_iter().map(KadPeer::into).collect();
                let fixed_query = FixedQuery::new(QueryType::AddProvider { provider, addresses }, messengers, config, peers);
                fixed_query.run(f);
            }
        });
    }

    /// Stops the local node from announcing that it is a provider for the given key.
    ///
    /// This is a local operation. The local node will still be considered as a
    /// provider for the key by other nodes until these provider records expire.
    fn stop_providing(&mut self, key: &record::Key) {
        self.store.remove_provider(key, self.kbuckets.self_key().preimage());
    }

    /// Finds the closest peers to a `target` in the context of a request by
    /// the `source` peer, such that the `source` peer is never included in the
    /// result.
    fn find_closest<T: Clone>(&mut self, target: &kbucket::Key<T>, source: &PeerId) -> Vec<KadPeer> {
        if target == self.kbuckets.self_key() {
            vec![KadPeer {
                node_id: self.kbuckets.self_key().preimage().clone(),
                multiaddrs: vec![], // don't bother, they must have our addresses
                connection_ty: KadConnectionType::Connected,
            }]
        } else {
            let connected = &self.connected_peers;
            let swarm = self.swarm.as_ref().expect("must be Some");
            self.kbuckets
                .closest(target)
                .filter(|e| e.node.key.preimage() != source)
                .take(self.query_config.k_value.get())
                .map(|n| {
                    let node_id = n.node.key.into_preimage();
                    let connection_ty = if connected.contains(&node_id) {
                        KadConnectionType::Connected
                    } else {
                        KadConnectionType::NotConnected
                    };
                    let multiaddrs = swarm.get_addrs(&node_id).unwrap_or_default();
                    // Note: here might be a possibility that multiaddrs is empty, if the peerstore doesn't have
                    // the address info for some reason(BUG?). Therefore, we'll send out a Kad peer without addresses.
                    // Shall we or shall we not???
                    KadPeer {
                        node_id,
                        multiaddrs,
                        connection_ty,
                    }
                })
                .collect()
        }
    }

    /// Collects all peers who are known to be providers of the value for a given `Multihash`.
    fn provider_peers(&mut self, key: &record::Key, source: Option<&PeerId>) -> Vec<KadPeer> {
        let kbuckets = &mut self.kbuckets;
        let connected = &self.connected_peers;
        let local_addrs = &self.local_addrs;
        let swarm = self.swarm.as_ref().expect("must be Some");
        self.store
            .providers(key)
            .into_iter()
            .filter_map(move |p| {
                if source.map_or(true, |id| id != &p.provider) {
                    let node_id = p.provider;
                    let connection_ty = if connected.contains(&node_id) {
                        KadConnectionType::Connected
                    } else {
                        KadConnectionType::NotConnected
                    };
                    // The provider is either the local node and we fill in
                    // the local addresses on demand, or it is a legacy
                    // provider record without addresses, in which case we
                    // try to find addresses in the routing table, as was
                    // done before provider records were stored along with
                    // their addresses.
                    let multiaddrs = if &node_id == kbuckets.self_key().preimage() {
                        Some(local_addrs.clone())
                    } else {
                        swarm.get_addrs(&node_id)
                    }
                    .unwrap_or_default();

                    Some(KadPeer {
                        node_id,
                        multiaddrs,
                        connection_ty,
                    })
                } else {
                    None
                }
            })
            .take(self.query_config.k_value.get())
            .collect()
    }
    /*
        /// Starts an iterative `ADD_PROVIDER` query for the given key.
        fn start_add_provider(&mut self, key: record::Key, context: AddProviderContext) {
            let info = QueryInfo::AddProvider {
                context,
                key: key.clone(),
                //phase: AddProviderPhase::GetClosestPeers
                provider_id: PeerId::random(),
                external_addresses: vec![],
                get_closest_peers_stats: QueryStats::empty()
            };
            let target = kbucket::Key::new(key);
            let peers = self.kbuckets.closest_keys(&target);
            let inner = QueryInner::new(info);
            self.queries.add_iter_closest(target.clone(), peers, inner);
        }

        /// Starts an iterative `PUT_VALUE` query for the given record.
        fn start_put_record(&mut self, record: Record, context: PutRecordContext) {
            let quorum = self.query_config.replication_factor;
            let target = kbucket::Key::new(record.key.clone());
            let peers = self.kbuckets.closest_keys(&target);
            let info = QueryInfo::PutRecord {
                record, quorum, context//, phase: PutRecordPhase::GetClosestPeers
            };
            let inner = QueryInner::new(info);
            self.queries.add_iter_closest(target.clone(), peers, inner);
        }
    */
    /// Processes a record received from a peer.
    fn handle_put_record(&mut self, _source: PeerId, mut record: Record) -> Result<KadResponseMsg> {
        if record.publisher.as_ref() == Some(self.kbuckets.self_key().preimage()) {
            // If the (alleged) publisher is the local node, do nothing. The record of
            // the original publisher should never change as a result of replication
            // and the publisher is always assumed to have the "right" value.
            return Ok(KadResponseMsg::PutValue {
                key: record.key,
                value: record.value,
            });
        }

        let now = Instant::now();

        // Calculate the expiration exponentially inversely proportional to the
        // number of nodes between the local node and the closest node to the key
        // (beyond the replication factor). This ensures avoiding over-caching
        // outside of the k closest nodes to a key.
        let target = kbucket::Key::new(record.key.clone());
        let num_between = self.kbuckets.count_nodes_between(&target);
        let k = self.query_config.k_value.get();
        let num_beyond_k = (usize::max(k, num_between) - k) as u32;
        let expiration = self.record_ttl.map(|ttl| now + exp_decrease(ttl, num_beyond_k));
        // The smaller TTL prevails. Only if neither TTL is set is the record
        // stored "forever".
        record.expires = record.expires.or(expiration).min(expiration);

        // While records received from a publisher, as well as records that do
        // not exist locally should always (attempted to) be stored, there is a
        // choice here w.r.t. the handling of replicated records whose keys refer
        // to records that exist locally: The value and / or the publisher may
        // either be overridden or left unchanged. At the moment and in the
        // absence of a decisive argument for another option, both are always
        // overridden as it avoids having to load the existing record in the
        // first place.

        log::debug!("adding record to store: {:?}", record);

        if !record.is_expired(now) {
            // The record is cloned because of the weird libp2p protocol
            // requirement to send back the value in the response, although this
            // is a waste of resources.
            match self.store.put(record.clone()) {
                Ok(()) => log::debug!("Record stored: {:?}; {} bytes", record.key, record.value.len()),
                Err(e) => {
                    log::debug!("Record not stored: {:?}", e);
                    return Err(e);
                }
            }
        }

        // The remote receives a [`KademliaHandlerIn::PutRecordRes`] even in the
        // case where the record is discarded due to being expired. Given that
        // the remote sent the local node a [`ProtocolEvent::PutRecord`]
        // request, the remote perceives the local node as one node among the k
        // closest nodes to the target. In addition returning
        // [`KademliaHandlerIn::PutRecordRes`] does not reveal any internal
        // information to a possibly malicious remote node.
        Ok(KadResponseMsg::PutValue {
            key: record.key,
            value: record.value,
        })
    }

    /// Processes a provider record received from a peer.
    fn handle_add_provider(&mut self, key: record::Key, provider: KadPeer) {
        if &provider.node_id != self.kbuckets.self_key().preimage() {
            log::debug!("adding provider to store: {:?}", provider);
            // add provider's addresses to peerstore
            self.swarm
                .as_ref()
                .expect("must be Some")
                .add_addrs(&provider.node_id, provider.multiaddrs, PROVIDER_ADDR_TTL);

            let record = ProviderRecord::new(key, provider.node_id, self.provider_record_ttl.map(|ttl| Instant::now() + ttl));
            if let Err(e) = self.store.add_provider(record) {
                log::debug!("Provider record not stored: {:?}", e);
            }
        }
    }

    /// Get the protocol handler of Kademlia, swarm will call "handle" func after stream negotiation.
    pub fn handler(&self) -> KadProtocolHandler {
        KadProtocolHandler::new(self.protocol_config.clone(), self.poster())
    }
    /// Get the controller of Kademlia, which can be used to manipulate the Kad-DHT.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone())
    }

    fn start_provider_gc_timer(&mut self) {
        // start timer task, which would generate ProtocolEvent::ProviderCleanupTimer to kad main loop
        log::info!("starting provider timer task...");
        let interval = self.cleanup_interval;
        let mut poster = self.poster();
        let h = task::spawn(async move {
            loop {
                task::sleep(interval).await;
                let _ = poster.post(ProtocolEvent::ProviderCleanupTimer).await;
            }
        });

        self.provider_timer_handle = Some(h);
    }

    fn start_refresh_timer(&mut self) {
        if let Some(interval) = self.refresh_interval {
            // start timer task, which would generate ProtocolEvent::RefreshTimer to kad main loop
            log::info!("starting refresh timer task...");
            let mut poster = self.poster();
            let h = task::spawn(async move {
                loop {
                    task::sleep(interval).await;
                    let _ = poster.post(ProtocolEvent::RefreshTimer).await;
                }
            });

            self.refresh_timer_handle = Some(h);
        }
    }

    /// Start the main message loop of Kademlia.
    pub fn start(mut self, swarm: SwarmControl) {
        self.messengers = Some(MessengerManager::new(swarm.clone(), self.protocol_config.clone()));
        self.swarm = Some(swarm);

        // start provider gc timer
        self.start_provider_gc_timer();
        // start refresh timer
        self.start_refresh_timer();

        // well, self 'move' explicitly,
        let mut kad = self;
        task::spawn(async move {
            // // As we get Swarm control, try getting Swarm self addresses
            // let swarm = kad.swarm.as_mut().expect("must be Some");
            // kad.local_addrs = swarm.self_addrs().await.expect("listen addrs > 0");
            let r = kad.process_loop().await;
            assert!(r.is_err());
            log::info!("Kad main loop closed, quitting due to {:?}", r);

            if let Some(h) = kad.refresh_timer_handle.take() {
                h.cancel().await;
            }
            if let Some(h) = kad.provider_timer_handle.take() {
                h.cancel().await;
            }

            log::info!("Kad main loop exited");
        });
    }

    /// Message Process Loop.
    async fn process_loop(&mut self) -> Result<()> {
        loop {
            select! {
                evt = self.event_rx.next() => {
                    self.on_events(evt).await?;
                }
                cmd = self.control_rx.next() => {
                    self.on_control_command(cmd).await?;
                }
            }
        }
    }

    // Called when new peer is connected.
    fn handle_peer_connected(&mut self, peer_id: PeerId) {
        // the peer id might have existed in the hashset, don't care too much
        self.connected_peers.insert(peer_id);
    }

    // Called when a peer is disconnected.
    fn handle_peer_disconnected(&mut self, peer_id: PeerId) {
        // remove the peer from the hashset
        self.connected_peers.remove(&peer_id);
        // remove the cached messenger which belong to the disconnected connection
        // Note: self.connected_peers is a HashSet indexed by PeerId, but there is
        // a possibility that we have 2 connections to the remote peer. Thus, with
        // any peer disconnected notification, we remove the cached messenger anyway.
        // It is acceptable, as we can always re-open a messenger...
        if let Some(cache) = &mut self.messengers {
            cache.clear_messengers(&peer_id);
        }
    }

    // Called when a peer is identified.
    fn handle_peer_identified(&mut self, peer_id: PeerId) {
        // check if the peer is a eligible Kad peer, try add to Kad if it is
        if let Some(swarm) = &mut self.swarm {
            if swarm
                .first_supported_protocol(&peer_id, vec![self.protocol_config.protocol_name().to_string()])
                .is_some()
            {
                log::debug!("A peer identified as a qualified Kad peer: {:}", peer_id);
                self.try_add_peer(peer_id, false);
            }
        }
    }

    // handle a local address changes. Update the local_addrs and start a refresh immediately.
    fn handle_address_changed(&mut self, addrs: Vec<Multiaddr>) {
        log::debug!("address changed: {:?}, starting refresh...", addrs);
        self.local_addrs = addrs;
        // TODO: probably we should start a timer to trigger refreshing, to avoid refreshing too often
        self.handle_refresh_stage(RefreshStage::Start);
    }

    // handle a new Kad peer is found.
    fn handle_peer_found(&mut self, peer_id: PeerId, queried: bool) {
        self.try_add_peer(peer_id, queried);
    }

    // handle a Kad peer is dead.
    fn handle_peer_stopped(&mut self, peer_id: PeerId) {
        //self.try_remove_peer(peer_id);
        self.try_remove_peer(peer_id);
    }

    // handle iterative query completion
    fn handle_query_stats(&mut self, stats: QueryStats) {
        log::info!("iterative query report : {:?}", stats);

        // calculate the average duration...
        let total = self.stats.query.duration * self.stats.successful_queries as u32 + stats.duration;
        self.stats.successful_queries += 1;
        self.stats.query.duration = total / self.stats.successful_queries as u32;
        self.stats.query.merge(stats);
    }

    // handle iterative query timeout
    fn handle_query_timeout(&mut self) {
        self.stats.timeout_queries += 1;
    }

    // Handle Kad events sent from protocol handler.
    async fn on_events(&mut self, msg: Option<ProtocolEvent>) -> Result<()> {
        log::debug!("handle kad event: {:?}", msg);
        match msg {
            Some(ProtocolEvent::PeerConnected(peer_id)) => {
                self.handle_peer_connected(peer_id);
            }
            Some(ProtocolEvent::PeerDisconnected(peer_id)) => {
                self.handle_peer_disconnected(peer_id);
            }
            Some(ProtocolEvent::PeerIdentified(peer_id)) => {
                self.handle_peer_identified(peer_id);
            }
            Some(ProtocolEvent::AddressChanged(addrs)) => {
                self.handle_address_changed(addrs);
            }
            Some(ProtocolEvent::KadPeerFound(peer_id, queried)) => {
                self.handle_peer_found(peer_id, queried);
            }
            Some(ProtocolEvent::KadPeerStopped(peer_id)) => {
                self.handle_peer_stopped(peer_id);
            }
            Some(ProtocolEvent::IterativeQueryCompleted(stats)) => {
                self.handle_query_stats(stats);
            }
            Some(ProtocolEvent::IterativeQueryTimeout) => {
                self.handle_query_timeout();
            }
            Some(ProtocolEvent::KadRequest { request, source, reply }) => {
                self.handle_kad_request(request, source, reply);
            }
            Some(ProtocolEvent::ProviderCleanupTimer) => {
                self.handle_provider_cleanup();
            }
            Some(ProtocolEvent::RefreshTimer) => {
                self.handle_refresh_timer();
            }
            Some(ProtocolEvent::Refresh(stage)) => {
                self.handle_refresh_stage(stage);
            }
            None => {
                return Err(KadError::Closing(2));
            }
        }
        Ok(())
    }

    // Handles Kad request messages. ProtoBuf message decoded by handler.
    fn handle_kad_request(&mut self, request: KadRequestMsg, source: PeerId, reply: oneshot::Sender<Result<Option<KadResponseMsg>>>) {
        log::debug!("handle Kad request message from {:?}, {:?} ", source, request);

        // Obviously we found a Kad peer
        self.try_add_peer(source.clone(), true);

        let response = match request {
            KadRequestMsg::Ping => {
                self.stats.message_rx.ping += 1;
                // respond with the Pong message
                Ok(Some(KadResponseMsg::Pong))
            }
            KadRequestMsg::FindNode { key } => {
                self.stats.message_rx.find_node += 1;
                let closer_peers = self.find_closest(&kbucket::Key::new(key), &source);
                Ok(Some(KadResponseMsg::FindNode { closer_peers }))
            }
            KadRequestMsg::AddProvider { key, provider } => {
                self.stats.message_rx.add_provider += 1;
                // Only accept a provider record from a legitimate peer.
                if provider.node_id != source {
                    log::info!("received provider from wrong peer {:?}", source);
                    Err(KadError::InvalidSource(source))
                } else {
                    self.handle_add_provider(key, provider);
                    // AddProvider doesn't require a response
                    Ok(None)
                }
            }
            KadRequestMsg::GetProviders { key } => {
                self.stats.message_rx.get_provider += 1;
                let provider_peers = self.provider_peers(&key, Some(&source));
                let closer_peers = self.find_closest(&kbucket::Key::new(key), &source);
                Ok(Some(KadResponseMsg::GetProviders {
                    closer_peers,
                    provider_peers,
                }))
            }
            KadRequestMsg::GetValue { key } => {
                self.stats.message_rx.get_value += 1;
                // Lookup the record locally.
                let record = match self.store.get(&key) {
                    Some(record) => {
                        if record.is_expired(Instant::now()) {
                            self.store.remove(&key);
                            None
                        } else {
                            Some(record.into_owned())
                        }
                    }
                    None => None,
                };

                let closer_peers = self.find_closest(&kbucket::Key::new(key), &source);
                Ok(Some(KadResponseMsg::GetValue { record, closer_peers }))
            }
            KadRequestMsg::PutValue { record } => {
                self.stats.message_rx.put_value += 1;
                self.handle_put_record(source, record).map(Some)
            }
        };

        let _ = reply.send(response);
    }

    #[allow(clippy::needless_collect)]
    fn handle_provider_cleanup(&mut self) {
        // try to cleanup provider records
        let now = Instant::now();
        log::info!("handle_provider_cleanup, invoked at {:?}", now);

        // TODO: it is pretty confusing that self-owned Providers get recycled!!!
        //let provider_records = self.store.provided()
        let provider_records = self
            .store
            .all_providers()
            .filter(|r| r.is_expired(now))
            .map(|r| r.into_owned())
            .collect::<Vec<_>>();

        provider_records.into_iter().for_each(|r| {
            self.store.remove_provider(&r.key, &r.provider);
        });
    }

    // handle the timer to refresh the routing table. Actually it will trigger the
    // periodic bootstrap procedure in a fixed interval.
    fn handle_refresh_timer(&mut self) {
        let interval = self.check_kad_peer_interval;
        let now = Instant::now();
        let self_key = self.kbuckets.self_key().clone();
        let mut swarm = self.swarm.clone().expect("must be Some");
        let mut poster = self.poster();

        // collect all peers whose aliveness is older than check_kad_peer_interval
        let peers_to_check = self
            .kbuckets
            .closest(&self_key)
            .filter(|n| n.node.value.get_aliveness().map_or(true, |a| now.duration_since(a) > interval))
            .map(|n| n.node.key.into_preimage())
            .collect::<Vec<_>>();

        // start a task to do health check
        task::spawn(async move {
            // run healthy check for all nodes whose aliveness is older than check_kad_peer_interval
            log::debug!("about to health check {} nodes", peers_to_check.len());
            let mut count: u32 = 0;
            for peer in peers_to_check {
                log::debug!("health checking {}", peer);
                let r = swarm.new_connection(peer.clone()).await;
                if r.is_err() {
                    log::debug!("health checking failed at {}, removing from Kbuckets", peer);
                    count += 1;
                    let _ = poster.post(ProtocolEvent::KadPeerStopped(peer)).await;
                }
            }

            log::info!("Kad refresh restarted, total {} nodes removed from Kbuckets", count);
            let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::Start)).await;
        });
    }

    // When bootstrap is finished, we start a timer to refresh the routing table. Actually
    // it will trigger the periodic bootstrap procedure in a fixed interval.
    fn handle_refresh_stage(&mut self, stage: RefreshStage) {
        match stage {
            RefreshStage::Start => {
                // check if we are running a refresh. do NOT run again if yes
                if self.refreshing {
                    return;
                }
                log::debug!("start refreshing kbuckets...");

                // always mark refreshing as true if we step into this stage
                self.refreshing = true;
                // and increase the counter
                self.stats.total_refreshes += 1;

                let local_id = self.kbuckets.self_key().preimage().clone();
                let mut poster = self.poster();

                self.get_closest_peers(local_id.into(), |r| {
                    if r.is_err() {
                        log::info!("refresh get_closest_peers failed: {:?}", r);
                    }
                    task::spawn(async move {
                        let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::SelfQueryDone)).await;
                    });
                });
            }
            RefreshStage::SelfQueryDone => {
                log::debug!("bootstrap: self-query done, proceeding with random walk...");
                log::debug!("kbuckets entries={}", self.kbuckets.num_entries());

                // self.kbuckets.iter().for_each(|k|{
                //     if k.num_entries() > 0 {
                //         println!("{:?} : {}", k.index(), k.num_entries());
                //     }
                // });

                let self_key = self.kbuckets.self_key().clone();
                // The lookup for the local key finished. To complete the bootstrap process,
                // a bucket refresh should be performed for every bucket farther away than
                // the first non-empty bucket (which are most likely no more than the last
                // few, i.e. farthest, buckets).
                let peers = self.kbuckets.iter()
                    .skip_while(|b| b.is_empty())
                    .skip(1) // Skip the bucket with the closest neighbour.
                    .take(16)
                    .map(|b| {
                        // Try to find a key that falls into the bucket. While such keys can
                        // be generated fully deterministically, the current libp2p kademlia
                        // wire protocol requires transmission of the preimages of the actual
                        // keys in the DHT keyspace, hence for now this is just a "best effort"
                        // to find a key that hashes into a specific bucket. The probabilities
                        // of finding a key in the bucket `b` with as most 16 trials are as
                        // follows:
                        //
                        // Pr(bucket-255) = 1 - (1/2)^16   ~= 1
                        // Pr(bucket-254) = 1 - (3/4)^16   ~= 1
                        // Pr(bucket-253) = 1 - (7/8)^16   ~= 0.88
                        // Pr(bucket-252) = 1 - (15/16)^16 ~= 0.64
                        // ...
                        let mut target = kbucket::Key::new(PeerId::random());
                        for _ in 0..16 {
                            let d = self_key.distance(&target);
                            if b.contains(&d) {
                                log::trace!("random Id generated for bucket-index={:?}", d.ilog2());
                                break;
                            }
                            target = kbucket::Key::new(PeerId::random());
                        }
                        target.into_preimage()
                    }).collect::<Vec<_>>();

                log::debug!("random nodes generated: {:?}", peers);

                let mut control = self.control();
                let mut poster = self.poster();
                // start a separate task to do walking random Ids
                task::spawn(async move {
                    for peer in peers {
                        log::debug!("bootstrap: walk random node {:?}", peer);
                        let _ = control.lookup(peer.into()).await;
                    }
                    let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::Completed)).await;
                });
            }
            RefreshStage::Completed => {
                log::debug!("kbuckets entries={}", self.kbuckets.num_entries());
                log::info!("bootstrap: finished");
                // reset the refreshing flag
                self.refreshing = false;
            }
        }
    }

    // Process publish or subscribe command.
    async fn on_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::Bootstrap) => {
                self.bootstrap().await;
            }
            Some(ControlCommand::AddNode(peer, addresses)) => {
                self.add_node(peer, addresses);
            }
            Some(ControlCommand::RemoveNode(peer)) => {
                self.remove_node(peer);
            }
            Some(ControlCommand::Lookup(key, reply)) => {
                self.get_closest_peers(key, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::FindPeer(peer_id, reply)) => {
                self.find_peer(peer_id.into(), |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::FindProviders(key, count, reply)) => {
                self.get_providers(key, count, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::Providing(key, reply)) => {
                self.start_providing(key, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::PutValue(key, value, reply)) => {
                self.put_record(key, value, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::GetValue(key, reply)) => {
                self.get_record(key, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::Dump(cmd)) => match cmd {
                DumpCommand::Entries(reply) => {
                    let _ = reply.send(self.dump_kbuckets());
                }
                DumpCommand::Statistics(reply) => {
                    let _ = reply.send(self.dump_statistics());
                }
                DumpCommand::Messengers(reply) => {
                    let _ = reply.send(self.dump_messengers());
                }
            },
            None => {
                return Err(KadError::Closing(1));
            }
        }
        Ok(())
    }
}

/// Exponentially decrease the given duration (base 2).
fn exp_decrease(ttl: Duration, exp: u32) -> Duration {
    Duration::from_secs(ttl.as_secs().checked_shr(exp).unwrap_or(0))
}

///////////////////////////////////////////////////////////////////////////////////
#[derive(Clone)]
pub(crate) struct MessengerManager {
    swarm: SwarmControl,
    config: KademliaProtocolConfig,
    cache: Arc<Mutex<FnvHashMap<PeerId, KadMessenger>>>,
}

impl MessengerManager {
    fn new(swarm: SwarmControl, config: KademliaProtocolConfig) -> Self {
        Self {
            swarm,
            config,
            cache: Arc::new(Default::default()),
        }
    }

    pub(crate) fn swarm(&mut self) -> &mut SwarmControl {
        &mut self.swarm
    }

    pub(crate) async fn get_messenger(&mut self, peer: &PeerId) -> Result<KadMessenger> {
        // lock as little as possible
        let r = {
            let mut cache = self.cache.lock().unwrap();
            cache.remove(peer)
        };

        match r {
            Some(sender) => Ok(sender),
            None => {
                // make a new sender
                KadMessenger::build(self.swarm.clone(), peer.clone(), self.config.clone()).await
            }
        }
    }

    pub(crate) fn put_messenger(&mut self, mut messenger: KadMessenger) {
        if messenger.reuse() {
            let mut cache = self.cache.lock().unwrap();
            let peer = messenger.get_peer_id();

            // perhaps there is a messenger in the hashmap already
            if !cache.contains_key(peer) {
                cache.insert(peer.clone(), messenger);
            }
        }
    }

    pub(crate) fn clear_messengers(&mut self, peer_id: &PeerId) {
        let mut cache = self.cache.lock().unwrap();
        cache.remove(peer_id);
    }

    pub(crate) fn messengers(&self) -> Vec<KadMessengerView> {
        let cache = self.cache.lock().unwrap();
        cache.values().map(|m| m.to_view()).collect()
    }
}

/// A view/copy of a bucket in a [`KBucketsTable`].
#[derive(Debug)]
pub struct KBucketView {
    pub index: usize,
    pub bucket: Vec<KNodeView>,
}

#[derive(Debug)]
pub struct KNodeView {
    pub id: PeerId,
    pub aliveness: Option<Instant>,
    pub addresses: Vec<Multiaddr>,
    pub connected: bool,
}

impl fmt::Display for KNodeView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let now = Instant::now();
        write!(
            f,
            "{:52} Conn({}) {:?} Addrs({:?})",
            self.id,
            self.connected,
            self.aliveness.map(|a| now - a),
            self.addresses
        )
    }
}
