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

use crate::peerstore::AddrType::{KAD, OTHER};
use crate::{Multiaddr, PeerId, PublicKey};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fmt, io};

pub const ADDRESS_TTL: Duration = Duration::from_secs(60 * 60);
pub const TEMP_ADDR_TTL: Duration = Duration::from_secs(2 * 60);
pub const PROVIDER_ADDR_TTL: Duration = Duration::from_secs(10 * 60);
pub const RECENTLY_CONNECTED_ADDR_TTL: Duration = Duration::from_secs(10 * 60);
pub const OWN_OBSERVED_ADDR_TTL: Duration = Duration::from_secs(10 * 60);

pub const PERMANENT_ADDR_TTL: Duration = Duration::from_secs(u64::MAX - 1);
pub const CONNECTED_ADDR_TTL: Duration = Duration::from_secs(u64::MAX - 2);

pub const GC_PURGE_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Default, Clone)]
pub struct PeerStore {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default, Debug)]
pub struct Inner {
    addrs: AddrBook,
    protos: ProtoBook,
    keys: KeyBook,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerSaved {
    addr: Multiaddr,
    addr_type: AddrType,
    ttl: Duration,
}

impl PeerStore {
    /// Save addr_book when closing swarm
    pub fn save_data(&self) -> io::Result<()> {
        let mut ds_addr_book = HashMap::new();

        {
            let guard = self.inner.lock().unwrap();
            // Transfer peer_id to String and insert into a new HashMap
            for (peer_id, value) in guard.addrs.addr_book.iter() {
                let key = peer_id.to_string();
                let mut v = Vec::new();
                for item in value.to_vec() {
                    v.push(PeerSaved {
                        addr: item.addr,
                        addr_type: item.addr_type,
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

        let _ = file.read_exact(buf.as_mut())?;
        let json_data: HashMap<String, Vec<PeerSaved>> =
            serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let mut guard = self.inner.lock().unwrap();
        for (key, value) in json_data {
            let peer_id = PeerId::from_str(&key).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let mut v = Vec::new();
            for item in value {
                v.push(AddrBookRecord {
                    addr: item.addr,
                    addr_type: item.addr_type,
                    ttl: item.ttl,
                    expiry: Instant::now().checked_add(item.ttl),
                })
            }
            guard.addrs.addr_book.insert(peer_id, SmallVec::from(v));
        }

        println!("{:?}", guard.addrs);

        Ok(())
    }

    /// Insert a public key, indexed by peer_id.
    pub fn add_key(&self, peer_id: &PeerId, key: PublicKey) {
        let mut guard = self.inner.lock().unwrap();
        guard.keys.add_key(peer_id, key)
    }
    /// Delete public key by peer_id.
    pub fn del_key(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        guard.keys.del_key(peer_id);
    }

    /// Get public key by peer_id.
    pub fn get_key(&self, peer_id: &PeerId) -> Option<PublicKey> {
        let guard = self.inner.lock().unwrap();
        guard.keys.get_key(peer_id).cloned()
    }

    /// Get all peer Ids in peer store.
    pub fn get_all_peers(&self) -> Vec<PeerId> {
        let guard = self.inner.lock().unwrap();
        guard.addrs.get_all_peers()
    }

    /// Add address to address_book by peer_id, if exists, update rtt.
    pub fn add_addr(&self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration, is_kad: bool) {
        let mut guard = self.inner.lock().unwrap();
        guard.addrs.add_addr(peer_id, addr, ttl, is_kad);
    }

    /// Add many new addresses if they're not already in the Address Book.
    pub fn add_addrs(&self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: Duration, is_kad: bool) {
        let mut guard = self.inner.lock().unwrap();
        guard.addrs.add_addrs(peer_id, addrs, ttl, is_kad);
    }

    /// Delete all multiaddr of a peer from address book.
    pub fn clear_addrs(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        guard.addrs.clear_addrs(peer_id);
    }

    /// Retrieve the record from the address book.
    pub fn get_addrs(&self, peer_id: &PeerId) -> Option<SmallVec<[AddrBookRecord; 4]>> {
        let guard = self.inner.lock().unwrap();
        guard.addrs.get_addrs(peer_id).cloned()
    }

    /// Update ttl if current_ttl equals old_ttl.
    pub fn update_addr(&self, peer_id: &PeerId, new_ttl: Duration) {
        let mut guard = self.inner.lock().unwrap();
        guard.addrs.update_addr(peer_id, new_ttl);
    }

    /// Get smallvec by peer_id and remove expired address
    pub fn remove_expired_addr(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        guard.addrs.remove_expired_addr(peer_id)
    }

    /// Insert supported protocol by peer_id
    pub fn add_protocol(&self, peer_id: &PeerId, proto: Vec<String>) {
        let mut guard = self.inner.lock().unwrap();
        guard.protos.add_protocol(peer_id, proto);
    }

    /// Remove support protocol by peer_id
    pub fn remove_protocol(&self, peer_id: &PeerId) {
        let mut guard = self.inner.lock().unwrap();
        guard.protos.remove_protocol(peer_id);
    }

    /// Get supported protocol by peer_id.
    pub fn get_protocol(&self, peer_id: &PeerId) -> Option<Vec<String>> {
        let guard = self.inner.lock().unwrap();
        guard.protos.get_protocol(peer_id)
    }

    /// Get the first protocol which is matched by the given protocols.
    pub fn first_supported_protocol(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<String> {
        let guard = self.inner.lock().unwrap();
        guard.protos.first_supported_protocol(peer_id, proto)
    }

    /// Search all protocols and return an option that matches by given proto param.
    pub fn support_protocols(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<Vec<String>> {
        let guard = self.inner.lock().unwrap();
        guard.protos.support_protocols(peer_id, proto)
    }

    /// Remove timeout address
    pub async fn addr_gc(self) {
        loop {
            log::info!("GC is looping...");
            async_std::task::sleep(GC_PURGE_INTERVAL).await;
            let pid_addr = self.get_all_peers();
            if !pid_addr.is_empty() {
                for id in pid_addr {
                    self.remove_expired_addr(&id);
                }
            }
            log::info!("GC finished");
        }
    }
}

impl fmt::Debug for PeerStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PeerStore").field(&self.inner).finish()
    }
}

#[derive(Copy, Clone, PartialOrd, PartialEq, Debug, Serialize, Deserialize)]
pub enum AddrType {
    // KAD means that address is comes from kad protocol.
    // We can't delete kad address in gc, because it may used later.
    KAD,

    // Normal address, it will be deleted if timeout.
    OTHER,
}

/// Store address
#[derive(Default, Clone)]
struct AddrBook {
    addr_book: HashMap<PeerId, SmallVec<[AddrBookRecord; 4]>>,
}

/// Store address, time-to-server, and expired time
#[derive(Clone, Debug)]
pub struct AddrBookRecord {
    addr: Multiaddr,
    addr_type: AddrType,
    ttl: Duration,
    expiry: Option<Instant>,
}

impl AddrBookRecord {
    /// Set the route-trip-time
    pub fn get_addr(&self) -> &Multiaddr {
        &self.addr
    }

    /// Set the route-trip-time
    pub fn into_maddr(self) -> Multiaddr {
        self.addr
    }

    /// Set the route-trip-time
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl
    }

    /// Set the expiry time
    pub fn set_expiry(&mut self, expiry: Option<Instant>) {
        self.expiry = expiry
    }

    /// Get the route-trip-time
    pub fn get_type(&self) -> AddrType {
        self.addr_type
    }

    /// Set the type of address
    pub fn set_type(&mut self, addr_type: AddrType) {
        self.addr_type = addr_type
    }

    /// Get the expiry time
    pub fn get_expiry(&self) -> Option<Instant> {
        self.expiry
    }
}

impl fmt::Debug for AddrBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AddrBook").field(&self.addr_book).finish()
    }
}

impl fmt::Display for AddrBook {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        //self.addr_book.iter().for_each(|a| a.0.fmt(f)
        Ok(())
    }
}

impl AddrBook {
    // Add address to address_book by peer_id, if exists, update rtt.
    fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration, is_kad: bool) {
        // KAD address will never time out.
        let expiry = if is_kad { None } else { Instant::now().checked_add(ttl) };

        let addr_type = if is_kad { KAD } else { OTHER };
        // Peer_id exist, get vector.
        if let Some(entry) = self.addr_book.get_mut(peer_id) {
            let mut exist = false;

            // Update address's expiry if exist.
            for (count, i) in entry.iter().enumerate() {
                if i.addr == addr {
                    let record: &mut AddrBookRecord = entry.get_mut(count).unwrap();

                    if is_kad {
                        // Update addr to KAD addr.
                        record.set_type(KAD);
                        record.set_expiry(None);
                    } else {
                        record.set_expiry(expiry);
                    }
                    exist = true;
                    break;
                }
            }

            // If not exists, insert an address into vector.
            if !exist {
                entry.push(AddrBookRecord {
                    addr,
                    addr_type,
                    ttl,
                    expiry,
                })
            }
        } else {
            // Peer_id non-exists, create a new vector.
            let vec = vec![AddrBookRecord {
                addr,
                addr_type,
                ttl,
                expiry,
            }];
            self.addr_book.insert(peer_id.clone(), SmallVec::from_vec(vec));
        }
    }

    fn add_addrs(&mut self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: Duration, is_kad: bool) {
        for addr in addrs {
            self.add_addr(peer_id, addr, ttl, is_kad)
        }
    }

    fn clear_addrs(&mut self, peer_id: &PeerId) {
        self.addr_book.remove(peer_id);
    }

    fn get_addrs(&self, peer_id: &PeerId) -> Option<&SmallVec<[AddrBookRecord; 4]>> {
        self.addr_book.get(peer_id)
    }

    fn get_all_peers(&self) -> Vec<PeerId> {
        self.addr_book.keys().cloned().collect()
    }

    // Update ttl if current_ttl equals old_ttl.
    fn update_addr(&mut self, peer_id: &PeerId, new_ttl: Duration) {
        if let Some(record_vec) = self.addr_book.get_mut(peer_id) {
            let time = Instant::now().checked_add(new_ttl);
            for record in record_vec.iter_mut() {
                if record.addr_type == KAD {
                    continue;
                }
                record.set_expiry(time);
            }
        }
    }

    // Get smallvec by peer_id and remove expired address
    pub fn remove_expired_addr(&mut self, peer_id: &PeerId) {
        let addr = self.addr_book.get_mut(peer_id).unwrap();
        let iter_vec = addr.clone();
        let mut remove_count = 0;
        for (index, value) in iter_vec.iter().enumerate() {
            if value.addr_type == KAD {
                continue;
            }
            if value.expiry.map_or(PERMANENT_ADDR_TTL, |d| d.elapsed()) < GC_PURGE_INTERVAL {
                continue;
            } else {
                addr.remove(index - remove_count);
                remove_count += 1;
            }
        }
    }
}

/// Retrieve public_key by peer_id.
///
/// As we all known, we can use public_key to obtain peer_id, but can't do it inversely.
#[derive(Default, Clone, Debug)]
struct KeyBook {
    key_book: HashMap<PeerId, PublicKey>,
}

impl KeyBook {
    // Insert public key by peer_id, if it is not there.
    fn add_key(&mut self, peer_id: &PeerId, key: PublicKey) {
        self.key_book.entry(peer_id.clone()).or_insert(key);
    }

    // Delete public key by peer_id.
    fn del_key(&mut self, peer_id: &PeerId) {
        self.key_book.remove(peer_id);
    }

    // Get public key by peer_id.
    fn get_key(&self, peer_id: &PeerId) -> Option<&PublicKey> {
        self.key_book.get(peer_id)
    }
}

/// Store all protocols that the peer supports.
#[derive(Default, Clone)]
struct ProtoBook {
    proto_book: HashMap<PeerId, HashSet<String>>,
}

impl fmt::Debug for ProtoBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProtoBook").field(&self.proto_book).finish()
    }
}

impl fmt::Display for ProtoBook {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        //self.addr_book.iter().for_each(|a| a.0.fmt(f)
        Ok(())
    }
}

impl ProtoBook {
    /// Insert support protocol by peer_id
    fn add_protocol(&mut self, peer_id: &PeerId, proto: Vec<String>) {
        if let Some(s) = self.proto_book.get_mut(peer_id) {
            for item in proto {
                s.insert(item);
            }
        } else {
            let mut s = HashSet::new();
            for item in proto {
                s.insert(item);
            }
            self.proto_book.insert(peer_id.clone(), s);
        }
    }

    /// Remove support protocol by peer_id
    fn remove_protocol(&mut self, peer_id: &PeerId) {
        log::info!("remove protocol");
        self.proto_book.remove(peer_id);
    }

    fn get_protocol(&self, peer_id: &PeerId) -> Option<Vec<String>> {
        match self.proto_book.get(peer_id) {
            Some(set) => {
                let mut result = Vec::<String>::new();
                for s in set.iter() {
                    result.push(s.parse().unwrap())
                }
                Some(result)
            }
            None => None,
        }
    }

    /// Get the first protocol which matched by given protocols
    fn first_supported_protocol(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<String> {
        match self.proto_book.get(peer_id) {
            Some(s) => {
                for item in proto {
                    if s.contains(&item) {
                        return Some(item);
                    }
                }
                None
            }
            None => None,
        }
    }

    /// Search all protocols and return an option that matches by given proto param
    fn support_protocols(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<Vec<String>> {
        match self.proto_book.get(peer_id) {
            Some(s) => {
                let mut proto_list = Vec::new();
                for item in proto {
                    if s.contains(&item) {
                        proto_list.push(item)
                    }
                }
                Some(proto_list)
            }
            None => None,
        }
    }

    #[allow(dead_code)]
    fn get_iter(&self) -> (Vec<PeerId>, Vec<String>) {
        let mut peer = vec![];
        let mut proto = vec![];
        for (k, v) in self.proto_book.iter() {
            peer.push(k.clone());
            for key in v.iter() {
                if !proto.contains(key) {
                    proto.push(key.clone())
                }
            }
        }
        (peer, proto)
    }
}

#[cfg(test)]
mod tests {
    use crate::peerstore::{AddrBook, ProtoBook};
    use crate::PeerId;
    use std::time::Duration;

    #[test]
    fn addr_book_basic() {
        //env_logger::from_env(env_logger::Env::default().default_filter_or("trace")).init();
        let mut ab = AddrBook::default();

        let peer_id = PeerId::random();

        ab.add_addr(&peer_id, "/memory/123456".parse().unwrap(), Duration::from_secs(1), false);

        assert_eq!(
            &(ab.get_addrs(&peer_id).unwrap().first().unwrap().addr),
            &"/memory/123456".parse().unwrap()
        );

        ab.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1), false);
        let addrs = ab.get_addrs(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1), false);
        let addrs = ab.get_addrs(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.clear_addrs(&peer_id);
        assert!(ab.get_addrs(&peer_id).is_none());
    }

    #[test]
    fn proto_book_basic() {
        //env_logger::from_env(env_logger::Env::default().default_filter_or("trace")).init();
        let mut proto = ProtoBook::default();
        let peer_id = PeerId::random();

        let proto_list = vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];
        proto.add_protocol(&peer_id, proto_list.clone());

        let p = proto.get_protocol(&peer_id).unwrap();

        for i in proto_list {
            if p.contains(&i) {
                continue;
            } else {
                unreachable!()
            }
        }

        let optional_list = vec!["/libp2p/noise/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()];
        let protocol = proto.first_supported_protocol(&peer_id, optional_list);
        assert_eq!(protocol.unwrap(), "/libp2p/yamux/1.0.0");

        let option_support_list = vec![
            "/libp2p/secio/1.0.0".to_string(),
            "/libp2p/noise/1.0.0".to_string(),
            "/libp2p/yamux/1.0.0".to_string(),
        ];
        let support_protocol = proto.support_protocols(&peer_id, option_support_list);
        assert_eq!(
            support_protocol.unwrap(),
            vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()]
        );
    }
}
