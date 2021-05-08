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

use libp2prs_core::{Multiaddr, PeerId, ProtocolId};
use libp2prs_runtime::task;
use libp2prs_swarm::Control as SwarmControl;

use crate::control::{Control, ControlCommand, DumpCommand};
use crate::protocol::{
    KadConnectionType, KadMessenger, KadMessengerView, KadPeer, KadProtocolHandler, KadRequestMsg, KadResponseMsg,
    KademliaProtocolConfig, ProtocolEvent, RefreshStage,
};

use crate::addresses::PeerInfo;
use crate::kbucket::KBucketsTable;
use crate::query::{FixedQuery, IterativeQuery, PeerRecord, QueryConfig, QueryStats, QueryStatsAtomic, QueryType};
use crate::store::RecordStore;
use crate::{kbucket, record, KadError, ProviderRecord, Record};
use libp2prs_core::peerstore::{ADDRESS_TTL, PROVIDER_ADDR_TTL};
use libp2prs_swarm::protocol_handler::{IProtocolHandler, ProtocolImpl};

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

    /// The statistics of iterative and fixed queries.
    query_stats: Arc<QueryStatsAtomic>,

    /// The statistics of Kad DHT.
    stats: KademliaStats,

    /// The cache manager of Kademlia messenger.
    messengers: Option<MessengerManager>,

    /// The currently connected peers.
    ///
    /// This is a superset of the connected peers currently in the routing table.
    connected_peers: FnvHashSet<PeerId>,

    /// The timer runtime handle of Provider cleanup job.
    provider_timer_handle: Option<task::TaskHandle<()>>,

    /// The timer runtime handle of Provider cleanup job.
    refresh_timer_handle: Option<task::TaskHandle<()>>,

    /// The periodic interval to cleanup expired providers & records.
    gc_interval: Duration,

    /// The periodic interval to refreshing the routing table.
    /// `None` disables auto refresh.
    refresh_interval: Option<Duration>,

    /// The TTL of regular (value-)records.
    record_ttl: Duration,

    /// The TTL of provider records.
    provider_ttl: Duration,

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

#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub(crate) provider: Vec<ProviderRecord>,
    pub(crate) record: Vec<Record>,
}

/// The configuration for the `Kademlia` behaviour.
///
/// The configuration is consumed by [`Kademlia::new`].
#[derive(Debug, Clone)]
pub struct KademliaConfig {
    query_config: QueryConfig,
    protocol_config: KademliaProtocolConfig,
    refresh_interval: Option<Duration>,
    gc_interval: Duration,
    record_ttl: Duration,
    provider_ttl: Duration,
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
            gc_interval: Duration::from_secs(12 * 60 * 60),
            record_ttl: Duration::from_secs(36 * 60 * 60),
            provider_ttl: Duration::from_secs(24 * 60 * 60),
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
        self.gc_interval = interval;
        self
    }

    /// Sets the TTL for stored records.
    ///
    /// The TTL should be significantly longer than the (re-)publication
    /// interval, to avoid premature expiration of records. The default is 36
    /// hours.
    ///
    /// Does not apply to provider records.
    pub fn with_record_ttl(mut self, record_ttl: Duration) -> Self {
        self.record_ttl = record_ttl;
        self
    }

    /// Sets the TTL for provider records.
    ///
    /// Must be significantly larger than the provider publication interval.
    pub fn with_provider_record_ttl(mut self, ttl: Duration) -> Self {
        self.provider_ttl = ttl;
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
    pub(crate) fn unbounded_post(&mut self, event: ProtocolEvent) -> Result<()> {
        self.0.unbounded_send(event).map_err(|e| e.into_send_error())?;
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
        let local_key = kbucket::Key::from(id);

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
            query_stats: Arc::new(Default::default()),
            stats: Default::default(),
            messengers: None,
            connected_peers: Default::default(),
            provider_timer_handle: None,
            refresh_timer_handle: None,
            gc_interval: config.gc_interval,
            refresh_interval: config.refresh_interval,
            record_ttl: config.record_ttl,
            provider_ttl: config.provider_ttl,
            check_kad_peer_interval: config.check_kad_peer_interval,
            local_addrs: vec![],
        }
    }

    // Returns a copied instance of Kad poster.
    fn poster(&self) -> KadPoster {
        KadPoster::new(self.event_tx.clone())
    }

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
    ///
    /// queried: it is for sure the node is alive at this moment, so a aliveness
    ///         timestamp will be generated for the node.
    /// permanent: node will be added as permanent node, which can not be removed by health check.  
    ///
    fn try_add_peer(&mut self, peer: PeerId, queried: bool, permanent: bool) {
        let timeout = self.check_kad_peer_interval;
        let now = Instant::now();
        let key = kbucket::Key::from(peer);

        log::debug!(
            "trying to add a peer: {:?} bucket-index={:?}, query={}, permanent={}",
            peer,
            self.kbuckets.bucket_index(&key),
            queried,
            permanent
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
                let info = PeerInfo::new(queried, permanent);
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
                        .filter(|n| {
                            !n.value.is_permanent() && n.value.get_aliveness().map_or(true, |a| now.duration_since(a) > timeout)
                        })
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
    fn try_remove_peer(&mut self, peer: PeerId, forced: bool) -> Option<kbucket::EntryView<kbucket::Key<PeerId>, PeerInfo>> {
        let key = kbucket::Key::from(peer);

        log::debug!(
            "trying to remove a peer: {:?} bucket-index={:?}, forced={}",
            peer,
            self.kbuckets.bucket_index(&key),
            forced
        );

        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry) => {
                if forced || !entry.value().is_permanent() {
                    // unpin the peer, so GC can run normally
                    if let Some(s) = self.swarm.as_ref() {
                        s.unpin(&peer)
                    }
                    log::debug!("removed {:?}", entry.value());
                    Some(entry.remove())
                } else {
                    entry.value().set_aliveness(None);
                    None
                }
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
        let key = kbucket::Key::from(peer);

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
        let local_id = *self.kbuckets.self_key().preimage();
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
            self.query_stats.clone(),
        )
    }

    /// Initiates an iterative lookup for the closest peers to the given key.
    fn get_closest_peers<F>(&mut self, key: record::Key, f: F)
    where
        F: FnOnce(Result<Vec<KadPeer>>) + Send + 'static,
    {
        log::debug!("finding closest peers {:?}", key);

        let q = self.prepare_iterative_query(QueryType::GetClosestPeers, key);

        q.run(|r| {
            f(r.and_then(|r| r.closest_peers.ok_or(KadError::NotFound)));
        });
    }

    /// Performs a lookup for the given PeerId.
    ///
    /// It will perform a lookup on the local peer store and then an iterative
    /// query on the Kad-DHT network if it has to.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<Option<KadPeer>>).
    fn find_peer<F>(&mut self, peer_id: PeerId, f: F)
    where
        F: FnOnce(Result<KadPeer>) + Send + 'static,
    {
        log::debug!("finding peer {:?}", peer_id);

        // check the local peer store for the given peer_id
        if let Some(s) = self.swarm.as_ref() {
            if let Some(addrs) = s.get_addrs(&peer_id) {
                f(Ok(KadPeer {
                    node_id: peer_id,
                    multiaddrs: addrs,
                    connection_ty: KadConnectionType::NotConnected,
                }));
                return;
            }
        }

        let q = self.prepare_iterative_query(QueryType::FindPeer, peer_id.into());

        q.run(|r| {
            f(r.and_then(|r| r.found_peer.ok_or(KadError::NotFound)));
        });
    }

    /// Performs a lookup for providers of a value to the given key.
    ///
    /// count: How many providers are needed. 0 means to lookup the DHT to find as
    /// many as possible.
    ///
    /// The result of this operation is delivered into the callback
    /// Fn(Result<Vec<KadPeer>>).
    fn find_providers<F>(&mut self, key: record::Key, count: usize, f: F)
    where
        F: FnOnce(Result<Vec<KadPeer>>) + Send + 'static,
    {
        log::debug!("finding providers {:?}", key);

        let provider_peers = self.provider_peers(&key, None);

        if count != 0 && provider_peers.len() >= count {
            // ok, we have enough providers for this key, simply return
            f(Ok(provider_peers));
        } else {
            let local = if provider_peers.is_empty() { None } else { Some(provider_peers) };
            let q = self.prepare_iterative_query(QueryType::GetProviders { count, local }, key);

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
        log::debug!("getting record {:?}", key);

        let quorum = self.query_config.k_value.get();
        let mut records = Vec::with_capacity(quorum);

        if let Some(record) = self.store.get(&key) {
            records.push(PeerRecord {
                peer: None,
                record: record.into_owned(),
            });
        }

        if records.len() >= quorum {
            // ok, we have enough, simply return first item
            let record = records.first().cloned().map_or(Err(KadError::NotFound), Ok);
            f(record);
        } else {
            let config = self.query_config.clone();
            let messengers = self.messengers.clone().expect("must be Some");

            let local = if records.is_empty() { None } else { Some(records) };
            let q = self.prepare_iterative_query(QueryType::GetRecord { quorum, local }, key);
            let stats = self.query_stats.clone();
            q.run(|r| {
                f(r.and_then(|r| {
                    let record = r.records.as_ref().map(|r| r.first().cloned());
                    if let Some(Some(record)) = record.clone() {
                        if let Some(cache_peers) = r.cache_peers {
                            let record = record.record;
                            let fixed_query = FixedQuery::new(QueryType::PutRecord { record }, messengers, config, cache_peers, stats);
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
        log::debug!("putting record {:?}", key);

        // TODO: probably we should check if there is an old record with the same key?

        let publisher = Some(*self.kbuckets.self_key().preimage());
        let record = Record::new(key, value, true, publisher);

        if let Err(e) = self.store.put(record.clone()) {
            f(Err(e));
            return;
        }

        let config = self.query_config.clone();
        let messengers = self.messengers.clone().expect("must be Some");
        let stats = self.query_stats.clone();
        // initiate the iterative lookup for closest peers, which can be used to publish the record
        self.get_closest_peers(record.key.clone(), move |peers| {
            if let Err(e) = peers {
                f(Err(e));
            } else {
                let peers = peers.unwrap().into_iter().map(KadPeer::into).collect::<Vec<_>>();
                let fixed_query = FixedQuery::new(QueryType::PutRecord { record }, messengers, config, peers, stats);
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
    /// > **Note**: Bootstrapping requires at least one node in the `boot` list or at
    /// least one node of the DHT to be known.
    fn bootstrap(&mut self, boot: Vec<(PeerId, Multiaddr)>, reply: Option<oneshot::Sender<Result<()>>>) {
        // add bootstrap nodes to RT, and marked as Permanent
        for (node, addr) in boot {
            self.add_node(node, vec![addr]);
        }
        log::debug!("bootstrapping...");
        let mut poster = self.poster();
        let _ = poster.unbounded_post(ProtocolEvent::Refresh(RefreshStage::Start(reply)));
    }

    fn add_node(&mut self, peer: PeerId, addresses: Vec<Multiaddr>) {
        if let Some(s) = self.swarm.as_ref() {
            s.add_addrs(&peer, addresses, ADDRESS_TTL)
        }
        self.try_add_peer(peer, false, true);
    }

    fn remove_node(&mut self, peer: PeerId) {
        if let Some(s) = self.swarm.as_ref() {
            s.clear_addrs(&peer)
        }
        self.try_remove_peer(peer, true);
    }

    fn dump_messengers(&mut self) -> Vec<KadMessengerView> {
        self.messengers.as_ref().expect("must be Some").messengers()
    }

    fn dump_statistics(&mut self) -> KademliaStats {
        self.stats.query = self.query_stats.to_view();
        self.stats.clone()
    }

    // TODO:
    fn dump_storage(&mut self) -> StorageStats {
        let provider = self.store.all_providers().map(|item| item.into_owned()).collect();
        let record = self.store.all_records().map(|item| item.into_owned()).collect();
        StorageStats { provider, record }
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
                        let id = *n.node.key.preimage();
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
        log::debug!("start providing {:?}", key);

        let provider = ProviderRecord::new(key.clone(), *self.kbuckets.self_key().preimage(), true);
        if let Err(e) = self.store.add_provider(provider.clone()) {
            f(Err(e));
            return;
        }
        let config = self.query_config.clone();
        let messengers = self.messengers.clone().expect("must be Some");
        let addresses = self.local_addrs.clone();
        let stats = self.query_stats.clone();
        // initiate the iterative lookup for closest peers, which can be used to publish the record
        self.get_closest_peers(key, move |peers| {
            if let Err(e) = peers {
                f(Err(e));
            } else {
                let peers = peers.unwrap().into_iter().map(KadPeer::into).collect();
                let fixed_query = FixedQuery::new(QueryType::AddProvider { provider, addresses }, messengers, config, peers, stats);
                fixed_query.run(f);
            }
        });
    }

    /// Stops the local node from announcing that it is a provider for the given key.
    ///
    /// This is a local operation. The local node will still be considered as a
    /// provider for the key by other nodes until these provider records expire.
    fn stop_providing(&mut self, key: &record::Key) {
        log::debug!("stop providing {:?}", key);
        self.store.remove_provider(key, self.kbuckets.self_key().preimage());
    }

    /// Finds the closest peers to a `target` in the context of a request by
    /// the `source` peer, such that the `source` peer is never included in the
    /// result.
    fn find_closest<T: Clone>(&mut self, target: &kbucket::Key<T>, source: &PeerId) -> Vec<KadPeer> {
        if target == self.kbuckets.self_key() {
            vec![KadPeer {
                node_id: *self.kbuckets.self_key().preimage(),
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
    fn handle_put_record(&mut self, _source: PeerId, record: Record) -> Result<KadResponseMsg> {
        if record.publisher.as_ref() == Some(self.kbuckets.self_key().preimage()) {
            // If the (alleged) publisher is the local node, do nothing. The record of
            // the original publisher should never change as a result of replication
            // and the publisher is always assumed to have the "right" value.
            return Ok(KadResponseMsg::PutValue {
                key: record.key,
                value: record.value,
            });
        }

        // While records received from a publisher, as well as records that do
        // not exist locally should always (attempted to) be stored, there is a
        // choice here w.r.t. the handling of replicated records whose keys refer
        // to records that exist locally: The value and / or the publisher may
        // either be overridden or left unchanged. At the moment and in the
        // absence of a decisive argument for another option, both are always
        // overridden as it avoids having to load the existing record in the
        // first place.

        log::debug!("handle adding record to store: {:?}", record);

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
            log::debug!("handle adding provider to store: {:?}", provider);
            // add provider's addresses to peerstore
            self.swarm
                .as_ref()
                .expect("must be Some")
                .add_addrs(&provider.node_id, provider.multiaddrs, PROVIDER_ADDR_TTL);

            let record = ProviderRecord::new(key, provider.node_id, false);
            if let Err(e) = self.store.add_provider(record) {
                log::debug!("Provider record not stored: {:?}", e);
            }
        }
    }

    /// Get the controller of Kademlia, which can be used to manipulate the Kad-DHT.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone())
    }

    fn start_provider_gc_timer(&mut self) {
        // start timer runtime, which would generate ProtocolEvent::ProviderCleanupTimer to kad main loop
        log::info!("starting provider timer runtime...");
        let interval = self.gc_interval;
        let mut poster = self.poster();
        let h = task::spawn(async move {
            loop {
                task::sleep(interval).await;
                let _ = poster.post(ProtocolEvent::GCTimer).await;
            }
        });

        self.provider_timer_handle = Some(h);
    }

    fn start_refresh_timer(&mut self) {
        if let Some(interval) = self.refresh_interval {
            // start timer runtime, which would generate ProtocolEvent::RefreshTimer to kad main loop
            log::info!("starting refresh timer runtime...");
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

    /// Message Process Loop.
    async fn process_loop(&mut self) -> Result<()> {
        loop {
            select! {
                evt = self.event_rx.next() => {
                    self.on_events(evt)?;
                }
                cmd = self.control_rx.next() => {
                    self.on_control_command(cmd)?;
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
                self.try_add_peer(peer_id, false, false);
            }
        }
    }

    // handle a local address changes. Update the local_addrs and start a refresh immediately.
    fn handle_address_changed(&mut self, addrs: Vec<Multiaddr>) {
        log::debug!("address changed: {:?}, starting refresh...", addrs);
        self.local_addrs = addrs;
        // TODO: probably we should start a timer to trigger refreshing, to avoid refreshing too often
        self.handle_refresh_stage(RefreshStage::Start(None));
    }

    // handle a new Kad peer is found.
    fn handle_peer_found(&mut self, peer_id: PeerId, queried: bool) {
        self.try_add_peer(peer_id, queried, false);
    }

    // handle a Kad peer is dead.
    fn handle_peer_stopped(&mut self, peer_id: PeerId) {
        //self.try_remove_peer(peer_id);
        self.try_remove_peer(peer_id, false);
    }

    // Handle Kad events sent from protocol handler.
    fn on_events(&mut self, msg: Option<ProtocolEvent>) -> Result<()> {
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
            Some(ProtocolEvent::KadRequest { request, source, reply }) => {
                self.handle_kad_request(request, source, reply);
            }
            Some(ProtocolEvent::GCTimer) => {
                self.handle_gc_timer();
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

        // 2021.4.12 don't add to RT at this moment, since this node might be running
        // as a Client only. Actually it can be added anyhow if it is identified as
        // a Server node.
        //
        // // Obviously we found a potential Kad peer
        // self.try_add_peer(source, true, false);

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
                let record = self.store.get(&key).map(|r| r.into_owned());

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

    fn handle_gc_timer(&mut self) {
        log::info!("handle_gc_timer, GC invoked");
        // try to GC providers & records
        self.store.gc_records(self.record_ttl);
        self.store.gc_providers(self.provider_ttl);
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
            .filter(|n| {
                !n.node.value.is_permanent() && n.node.value.get_aliveness().map_or(true, |a| now.duration_since(a) > interval)
            })
            .map(|n| n.node.key.into_preimage())
            .collect::<Vec<_>>();

        // start a runtime to do health check
        task::spawn(async move {
            // run healthy check for all nodes whose aliveness is older than check_kad_peer_interval
            log::debug!("about to health check {} nodes", peers_to_check.len());
            let mut count: u32 = 0;
            for peer in peers_to_check {
                log::debug!("health checking {}", peer);
                let r = swarm.new_connection(peer).await;
                if r.is_err() {
                    log::debug!("health checking failed at {}, removing from Kbuckets", peer);
                    count += 1;
                    let _ = poster.post(ProtocolEvent::KadPeerStopped(peer)).await;
                }
            }

            log::info!("Kad refresh restarted, total {} nodes removed from Kbuckets", count);
            let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::Start(None))).await;
        });
    }

    // When bootstrap is finished, we start a timer to refresh the routing table. Actually
    // it will trigger the periodic bootstrap procedure in a fixed interval.
    fn handle_refresh_stage(&mut self, stage: RefreshStage) {
        match stage {
            RefreshStage::Start(reply) => {
                // check if we are in refreshing. do NOT run again if yes
                if self.refreshing {
                    log::debug!("Don't refresh when RT is being refreshed");
                    if let Some(tx) = reply {
                        let _ = tx.send(Err(KadError::Bootstrap));
                    }
                    return;
                }

                // check if there is any node in the RT. Do NOTHING if not.
                // This might happen when address change notifications come from
                // Notifiee trait, when Kad is initializing while Swarm is listening
                // on a multiaddr
                if self.kbuckets.num_entries() == 0 {
                    log::debug!("Don't refresh when RT has nothing yet");
                    if let Some(tx) = reply {
                        let _ = tx.send(Err(KadError::Bootstrap));
                    }
                    return;
                }

                log::debug!("start refreshing kbuckets...");

                // always mark refreshing as true if we step into this stage
                self.refreshing = true;
                // and increase the counter
                self.stats.total_refreshes += 1;

                let local_id = *self.kbuckets.self_key().preimage();
                let mut poster = self.poster();

                self.get_closest_peers(local_id.into(), |r| {
                    if r.is_err() {
                        log::info!("refresh get_closest_peers failed: {:?}", r);
                    }
                    task::spawn(async move {
                        let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::SelfQueryDone(reply))).await;
                    });
                });
            }
            RefreshStage::SelfQueryDone(reply) => {
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
                        let mut target = kbucket::Key::from(PeerId::random());
                        for _ in 0..16 {
                            let d = self_key.distance(&target);
                            if b.contains(&d) {
                                log::trace!("random Id generated for bucket-index={:?}", d.ilog2());
                                break;
                            }
                            target = kbucket::Key::from(PeerId::random());
                        }
                        target.into_preimage()
                    }).collect::<Vec<_>>();

                log::debug!("random nodes generated: {:?}", peers);

                let mut control = self.control();
                let mut poster = self.poster();
                // start a separate runtime to do walking random Ids
                task::spawn(async move {
                    for peer in peers {
                        log::debug!("bootstrap: walk random node {:?}", peer);
                        let _ = control.lookup(peer.into()).await;
                    }
                    let _ = poster.post(ProtocolEvent::Refresh(RefreshStage::Completed)).await;

                    // reply to API client
                    if let Some(tx) = reply {
                        let _ = tx.send(Ok(()));
                    }
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
    fn on_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::Bootstrap(boot, reply)) => {
                self.bootstrap(boot, reply);
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
                self.find_peer(peer_id, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::FindProviders(key, count, reply)) => {
                self.find_providers(key, count, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::Providing(key, reply)) => {
                self.start_providing(key, |r| {
                    let _ = reply.send(r);
                });
            }
            Some(ControlCommand::Unprovide(key)) => {
                self.stop_providing(&key);
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
                DumpCommand::Storage(reply) => {
                    let _ = reply.send(self.dump_storage());
                }
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

impl<TStore> ProtocolImpl for Kademlia<TStore>
where
    for<'a> TStore: RecordStore<'a> + Send + 'static,
{
    fn handlers(&self) -> Vec<IProtocolHandler> {
        let p = Box::new(KadProtocolHandler::new(self.protocol_config.clone(), self.poster()));
        vec![p]
    }

    fn start(mut self, swarm: SwarmControl) -> Option<task::TaskHandle<()>>
    where
        Self: Sized,
    {
        self.messengers = Some(MessengerManager::new(swarm.clone(), self.protocol_config.clone()));
        self.swarm = Some(swarm);

        // start provider gc timer
        self.start_provider_gc_timer();
        // start refresh timer
        self.start_refresh_timer();

        // well, self 'move' explicitly,
        let mut kad = self;
        Some(task::spawn(async move {
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
        }))
    }
}

// /// Exponentially decrease the given duration (base 2).
// fn exp_decrease(ttl: Duration, exp: u32) -> Duration {
//     Duration::from_secs(ttl.as_secs().checked_shr(exp).unwrap_or(0))
// }

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
                KadMessenger::build(self.swarm.clone(), *peer, self.config.clone()).await
            }
        }
    }

    pub(crate) fn put_messenger(&mut self, mut messenger: KadMessenger) {
        if messenger.reuse() {
            let mut cache = self.cache.lock().unwrap();
            let peer = messenger.get_peer_id();

            // perhaps there is a messenger in the hashmap already
            if !cache.contains_key(peer) {
                cache.insert(*peer, messenger);
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
