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

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::metricmap::MetricMap;
use crate::{PeerId, PublicKey};
use libp2prs_multiaddr::Multiaddr;

pub const ADDRESS_TTL: Duration = Duration::from_secs(60 * 60);
pub const TEMP_ADDR_TTL: Duration = Duration::from_secs(2 * 60);
pub const PROVIDER_ADDR_TTL: Duration = Duration::from_secs(10 * 60);
pub const RECENTLY_CONNECTED_ADDR_TTL: Duration = Duration::from_secs(10 * 60);
pub const OWN_OBSERVED_ADDR_TTL: Duration = Duration::from_secs(10 * 60);

pub const PERMANENT_ADDR_TTL: Duration = Duration::from_secs(u64::MAX - 1);
pub const CONNECTED_ADDR_TTL: Duration = Duration::from_secs(u64::MAX - 2);

pub const LATENCY_EWMA_SMOOTHING: u128 = 1 / 10;

#[derive(Default, Clone)]
pub struct PeerStore {
    inner: Arc<Mutex<HashMap<PeerId, PeerRecord>>>,
    m: Arc<MetricMap<PeerId, Duration>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerSaved {
    addr: Multiaddr,
    ttl: Duration,
}

/// The PeerInfo represents a remote peer and its elements.
#[derive(Clone)]
struct PeerRecord {
    /// Indicates if this record is currently pinned in peer store.
    ///
    /// PeerStore GC will not recycle a pinned record.
    pinned: bool,
    /// The multiaddr owned by this peer.
    addrs: Vec<AddrBookRecord>,
    /// The public key of the peer.
    key: Option<PublicKey>,
    /// The protocols supported by the peer.
    protos: HashSet<String>,
}

impl PeerRecord {
    fn new(addrs: Vec<AddrBookRecord>, key: Option<PublicKey>, protos: HashSet<String>) -> Self {
        Self {
            pinned: false,
            addrs,
            key,
            protos,
        }
    }
}

#[derive(Clone, Debug)]
struct AddrBookRecord {
    addr: Multiaddr,
    ttl: Duration,
    expiry: Instant,
}

impl Into<Multiaddr> for AddrBookRecord {
    fn into(self) -> Multiaddr {
        self.addr
    }
}

impl PeerStore {
    /// Save addr_book when closing swarm
    pub fn save_data(&self) -> io::Result<()> {
        let mut ds_addr_book = HashMap::new();

        {
            let guard = self.inner.lock().unwrap();
            // Transfer peer_id to String and insert into a new HashMap
            for (peer_id, value) in guard.iter() {
                let key = peer_id.to_string();
                let mut v = Vec::new();
                // save address info
                for item in value.addrs.to_vec() {
                    v.push(PeerSaved {
                        addr: item.addr,
                        ttl: item.ttl,
                    })
                }
                ds_addr_book.insert(key, v);
            }
        }
        let json_addrbook = serde_json::to_string(&ds_addr_book)?;

        let mut file = File::create("./ds_addr_book.txt")?;
        file.write_all(json_addrbook.as_bytes())
    }

    /// Load addr_book when initializing swarm
    pub fn load_data(&self) -> io::Result<()> {
        let mut file = match File::open("./ds_addr_book.txt") {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    File::create("./ds_addr_book.txt")?
                } else {
                    return Err(e);
                }
            }
        };
        let metadata = file.metadata()?;
        let length = metadata.len() as usize;
        if length == 0 {
            return Ok(());
        }
        let mut buf = vec![0u8; length];

        // Read data from file and deserialize
        let _ = file.read_exact(buf.as_mut())?;
        let json_data: HashMap<String, Vec<PeerSaved>> =
            serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Iter and insert into hashmap
        let mut guard = self.inner.lock().unwrap();
        for (key, value) in json_data {
            let peer_id = PeerId::from_str(&key).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let mut v = Vec::new();
            for item in value {
                v.push(AddrBookRecord::new(item.addr, item.ttl));
            }
            guard.insert(peer_id, PeerRecord::new(v, None, Default::default()));
        }

        Ok(())
    }

    /// Gets all peer Ids in peer store.
    pub fn get_peers(&self) -> Vec<PeerId> {
        let guard = self.inner.lock().unwrap();
        guard.keys().cloned().collect()
    }

    /// Pins the peer Id so that GC wouldn't recycle the multiaddr of the peer.
    pub fn pin(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.pinned = true;
        }
    }

    /// Unpins the peer Id.
    pub fn unpin(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.pinned = false;
        }
    }

    /// Checks if the peer is currently being pinned in peer store.
    pub fn pinned(&self, peer_id: &PeerId) -> bool {
        let guard = self.inner.lock().unwrap();
        guard.get(peer_id).map_or(false, |pr| pr.pinned)
    }

    /// Adds public key by peer_id.
    pub fn add_key(&self, peer_id: &PeerId, key: PublicKey) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.key = Some(key);
        }
    }

    /// Gets public key by peer_id.
    pub fn get_key(&self, peer_id: &PeerId) -> Option<PublicKey> {
        let guard = self.inner.lock().unwrap();
        guard.get(peer_id).and_then(|pr| pr.key.clone())
    }

    /// Add address to address_book by peer_id, if exists, update rtt.
    pub fn add_addr(&self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration) {
        self.add_addrs(peer_id, vec![addr], ttl)
    }

    /// Adds many new addresses if they're not already in the Address Book.
    pub fn add_addrs(&self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: Duration) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            for addr in addrs {
                if let Some(record) = pr.addrs.iter_mut().find(|item| item.addr == addr) {
                    // addr exists, update ttl & expiry
                    record.set_ttl(ttl);
                } else {
                    pr.addrs.push(AddrBookRecord::new(addr, ttl));
                }
            }
        } else {
            // Peer_id non-exists, create a new PeerRecord and fill with a new AddrBookRecord.
            let vec = addrs.into_iter().map(|addr| AddrBookRecord::new(addr, ttl)).collect();
            guard.insert(*peer_id, PeerRecord::new(vec, None, Default::default()));
        }
    }

    /// Removes all multiaddr of a peer from peer store.
    pub fn clear_addrs(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.addrs.clear();
        }
    }

    /// Retrieves the all multiaddr of a peer from the peer store.
    pub fn get_addrs(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        let guard = self.inner.lock().unwrap();
        guard.get(peer_id).map(|pr| pr.addrs.iter().map(|a| a.clone().into()).collect())
    }

    /// Updates the ttl of the multiaddr of the peer.
    pub fn update_addr(&self, peer_id: &PeerId, new_ttl: Duration) {
        let mut guard = self.inner.lock().unwrap();

        if let Some(pr) = guard.get_mut(peer_id) {
            for record in pr.addrs.iter_mut() {
                record.set_ttl(new_ttl);
            }
        }
    }

    /// Removes all expired address.
    pub fn remove_expired_addrs(&self) {
        let mut to_remove = vec![];
        let mut guard = self.inner.lock().unwrap();
        for (peer, pr) in guard.iter_mut() {
            if !pr.pinned {
                log::debug!("GC attempt for {:?}", peer);
                pr.addrs.retain(|record| record.expiry.elapsed() < record.ttl);
                // delete this peer if no addr at all
                if pr.addrs.is_empty() {
                    log::debug!("remove {:?} from peerstore", peer);
                    to_remove.push(*peer);
                }
            }
        }

        for peer in to_remove {
            guard.remove(&peer);
        }
    }

    /// Adds the supported protocols of a peer to the peer store.
    pub fn add_protocols(&self, peer_id: &PeerId, protos: Vec<String>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.protos.extend(protos);
        } else {
            let mut s = HashSet::new();
            s.extend(protos);
            guard.insert(*peer_id, PeerRecord::new(Default::default(), None, s));
        }
    }

    /// Clears the protocols by peer_id
    pub fn clear_protocols(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get_mut(peer_id) {
            pr.protos.clear();
        }
    }

    /// Gets the protocols by peer_id.
    pub fn get_protocols(&self, peer_id: &PeerId) -> Option<Vec<String>> {
        let guard = self.inner.lock().unwrap();
        guard.get(peer_id).map(|pr| pr.protos.iter().cloned().collect())
    }

    /// Get the first protocol which is matched by the given protocols.
    pub fn first_supported_protocol(&self, peer_id: &PeerId, protos: Vec<String>) -> Option<String> {
        let guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get(peer_id) {
            for proto in protos {
                if pr.protos.contains(&proto) {
                    return Some(proto);
                }
            }
        }
        None
    }

    /// Searches all protocols and return an option that matches by the given protocols.
    pub fn support_protocols(&self, peer_id: &PeerId, protos: Vec<String>) -> Option<Vec<String>> {
        let guard = self.inner.lock().unwrap();
        if let Some(pr) = guard.get(peer_id) {
            let mut proto_list = Vec::with_capacity(protos.len());
            for item in protos {
                if pr.protos.contains(&item) {
                    proto_list.push(item)
                }
            }
            Some(proto_list)
        } else {
            None
        }
    }

    /// Update rtt by peer_id
    pub fn record_latency(&self, peer_id: &PeerId, rtt: Duration) {
        self.m.store_or_modify(peer_id, rtt, |_, value| {
            let peer_rtt = (1 - LATENCY_EWMA_SMOOTHING) * value.clone().as_micros() + LATENCY_EWMA_SMOOTHING * rtt.as_micros();

            Duration::from_millis(peer_rtt as u64)
        });
    }

    /// Return latency info
    pub fn list_latency(&self) -> Vec<(PeerId, Duration)> {
        self.m.iterator().unwrap().collect::<Vec<_>>()
    }
}

#[allow(dead_code)]
impl AddrBookRecord {
    pub fn new(addr: Multiaddr, ttl: Duration) -> Self {
        Self {
            addr,
            ttl,
            expiry: Instant::now(),
        }
    }
    /// Get the multiaddr.
    pub fn get_addr(&self) -> &Multiaddr {
        &self.addr
    }

    /// Set the time-to-live. It would also reset the 'expiry'.
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl;
        self.expiry = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use crate::identity::Keypair;
    use crate::peerstore::{PeerStore, ADDRESS_TTL};
    use crate::PeerId;
    use libp2prs_multiaddr::Multiaddr;
    use std::time::Duration;

    #[test]
    fn addr_basic() {
        let keypair = Keypair::generate_secp256k1();
        let peer_id = PeerId::from_public_key(keypair.public());

        let peerstore = PeerStore::default();

        peerstore.add_key(&peer_id, keypair.public());
        peerstore.add_addr(&peer_id, "/memory/123456".parse().unwrap(), Duration::from_secs(1));

        assert_eq!(
            peerstore.get_addrs(&peer_id).unwrap().first().unwrap(),
            &"/memory/123456".parse::<Multiaddr>().unwrap()
        );

        peerstore.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1));
        let addrs = peerstore.get_addrs(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        peerstore.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1));
        let addrs = peerstore.get_addrs(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        peerstore.clear_addrs(&peer_id);
        assert_eq!(peerstore.get_addrs(&peer_id).unwrap().len(), 0);
    }

    #[test]
    fn proto_basic() {
        let keypair = Keypair::generate_secp256k1();
        let peer_id = PeerId::from_public_key(keypair.public());

        let peerstore = PeerStore::default();

        let proto_list = vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];

        peerstore.add_key(&peer_id, keypair.public());
        peerstore.add_protocols(&peer_id, proto_list.clone());

        let p = peerstore.get_protocols(&peer_id).unwrap();
        // let p = peerstore.get_protocol(&peer_id).unwrap();

        for i in proto_list {
            if p.contains(&i) {
                continue;
            } else {
                unreachable!()
            }
        }

        let optional_list = vec!["/libp2p/noise/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];
        let protocol = peerstore.first_supported_protocol(&peer_id, optional_list);
        assert_eq!(protocol.unwrap(), "/libp2p/yamux/1.0.0");

        let option_support_list = vec![
            "/libp2p/secio/1.0.0".to_string(),
            "/libp2p/noise/1.0.0".to_string(),
            "/libp2p/yamux/1.0.0".to_string(),
        ];
        let support_protocol = peerstore.support_protocols(&peer_id, option_support_list);
        assert_eq!(
            support_protocol.unwrap(),
            vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()]
        );
    }

    #[test]
    fn peerstore_basic() {
        let keypair = Keypair::generate_secp256k1();
        let peer_id = PeerId::from_public_key(keypair.public());

        let addrs = vec!["/memory/123456".parse().unwrap(), "/memory/123456".parse().unwrap()];
        let protos = vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];

        let ps = PeerStore::default();
        ps.add_key(&peer_id, keypair.public());
        ps.add_addrs(&peer_id, addrs, ADDRESS_TTL);
        ps.add_protocols(&peer_id, protos);

        let optional_list = vec!["/libp2p/noise/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];
        let protocol = ps.first_supported_protocol(&peer_id, optional_list);
        assert_eq!(protocol.unwrap(), "/libp2p/yamux/1.0.0");

        let option_support_list = vec![
            "/libp2p/secio/1.0.0".to_string(),
            "/libp2p/noise/1.0.0".to_string(),
            "/libp2p/yamux/1.0.0".to_string(),
        ];
        let support_protocol = ps.support_protocols(&peer_id, option_support_list);
        assert_eq!(
            support_protocol.unwrap(),
            vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()]
        );
    }

    #[test]
    fn peerstore_gc() {
        let peer_id = PeerId::random();
        let addrs = vec!["/memory/123456".parse().unwrap()];

        let ps = PeerStore::default();
        ps.add_addrs(&peer_id, addrs, Duration::from_secs(5));
        ps.pin(&peer_id);
        assert!(ps.get_addrs(&peer_id).is_some());

        std::thread::sleep(Duration::from_secs(5));
        ps.remove_expired_addrs();
        assert!(ps.get_addrs(&peer_id).is_some());

        ps.unpin(&peer_id);
        ps.remove_expired_addrs();
        assert!(ps.get_addrs(&peer_id).is_none());
    }
}
