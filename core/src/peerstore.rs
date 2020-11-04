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

use crate::{Multiaddr, PeerId, PublicKey};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use std::{fmt, io};

#[derive(Default, Clone)]
pub struct PeerStore {
    pub addrs: AddrBook,
    pub protos: ProtoBook,
    pub keys: KeyBook,
}

impl PeerStore {
    /// Save addr_book when closing swarm
    pub fn save_data(&self) -> io::Result<()> {
        let mut ds_addr_book = HashMap::new();
        // Transfer peer_id to String and insert into a new HashMap
        for (peer_id, value) in self.addrs.addr_book.clone() {
            let key = peer_id.to_string();
            ds_addr_book.insert(key, value.to_vec());
        }
        let json_addrbook = serde_json::to_string(&ds_addr_book)?;

        let mut file = File::create("./ds_addr_book.txt")?;
        file.write_all(json_addrbook.as_bytes())
    }

    /// Load addr_book when initializing swarm
    pub fn load_data(&mut self) -> io::Result<()> {
        let mut file = File::open("./ds_addr_book.txt")?;
        let metadata = file.metadata()?;
        let length = metadata.len() as usize;
        let mut buf = vec![0u8; length];

        let _ = file.read_exact(buf.as_mut())?;
        let json_data: HashMap<String, Vec<AddrBookRecord>> =
            serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        for (key, value) in json_data {
            let peer_id = PeerId::from_str(&key).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            self.addrs.addr_book.insert(peer_id, SmallVec::from(value));
        }

        Ok(())
    }
}

impl fmt::Debug for PeerStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PeerStore").field(&self.addrs).finish()
    }
}

impl fmt::Display for PeerStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.addrs.fmt(f)
    }
}

/// Store address
#[derive(Default, Clone)]
pub struct AddrBook {
    addr_book: HashMap<PeerId, SmallVec<[AddrBookRecord; 4]>>,
}

/// Store address, time-to-server, and expired time
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddrBookRecord {
    addr: Multiaddr,
    ttl: f64,
    expiry: Duration,
}

impl AddrBookRecord {
    /// Set the route-trip-time
    pub fn get_addr(&self) -> &Multiaddr {
        &self.addr
    }

    /// Set the route-trip-time
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl.as_secs_f64()
    }

    /// Set the expiry time
    pub fn set_expiry(&mut self, expiry: Duration) {
        self.expiry = expiry
    }

    /// Get the route-trip-time
    pub fn get_ttl(&self) -> f64 {
        self.ttl
    }

    /// Get the expiry time
    pub fn get_expiry(&self) -> Duration {
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
    /// Add address to address_book by peer_id, if exists, update rtt.
    pub fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration) {
        // Peer_id exist, get vector.
        if let Some(entry) = self.addr_book.get_mut(peer_id) {
            let mut exist = false;

            // Update address's expiry if exist.
            for (count, i) in entry.iter().enumerate() {
                if i.addr == addr {
                    // In order to get mutable
                    let record: &mut AddrBookRecord = entry.get_mut(count).unwrap();
                    let expiry = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .checked_add(ttl)
                        .unwrap();
                    record.set_expiry(expiry);
                    exist = true;
                    break;
                }
            }
            // If not exists, insert an address into vector.
            if !exist {
                entry.push(AddrBookRecord {
                    addr,
                    ttl: ttl.as_secs_f64(),
                    expiry: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .checked_add(ttl)
                        .unwrap(),
                })
            }
        } else {
            // Peer_id non-exists, create a new vector.
            let vec = vec![AddrBookRecord {
                addr,
                ttl: ttl.as_secs_f64(),
                expiry: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .checked_add(ttl)
                    .unwrap(),
            }];
            self.addr_book.insert(peer_id.clone(), SmallVec::from_vec(vec));
        }
    }

    pub fn del_peer(&mut self, peer_id: &PeerId) {
        self.addr_book.remove(peer_id);
    }

    pub fn get_addr(&self, peer_id: &PeerId) -> Option<&SmallVec<[AddrBookRecord; 4]>> {
        self.addr_book.get(peer_id)
    }

    /// Update ttl if current_ttl equals old_ttl.
    pub fn update_addr(&mut self, peer_id: &PeerId, old_ttl: Duration, new_ttl: Duration) {
        if self.get_addr(peer_id).is_some() {
            let record_vec = self.addr_book.get_mut(peer_id).unwrap();
            let time = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .checked_add(new_ttl)
                .unwrap();

            for record in record_vec.into_iter() {
                if (record.ttl - old_ttl.as_secs_f64()) == 0.0 {
                    record.set_ttl(new_ttl);
                    record.set_expiry(time);
                }
            }
        }
    }

    /// Get smallvec by peer_id and remove timeout address
    pub fn remove_timeout_addr(&mut self, peer_id: &PeerId) {
        let addr = self.addr_book.get_mut(peer_id).unwrap();
        let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let iter_vec = addr.clone();
        for (index, value) in iter_vec.iter().enumerate() {
            if value.expiry > time {
                continue;
            } else {
                addr.remove(index);
            }
        }
    }
}

/// Retrieve public_key by peer_id.
///
/// As we all known, we can use public_key to obtain peer_id, but can't do it inversely.
#[derive(Default, Clone)]
pub struct KeyBook {
    key_book: HashMap<PeerId, PublicKey>,
}

impl KeyBook {
    /// Insert public key by peer_id.
    pub fn add_key(&mut self, peer_id: &PeerId, key: PublicKey) {
        if self.key_book.get(peer_id).is_none() {
            self.key_book.insert(peer_id.clone(), key);
        }
    }

    /// Delete public key by peer_id.
    pub fn del_key(&mut self, peer_id: &PeerId) {
        self.key_book.remove(peer_id);
    }

    /// Get public key by peer_id.
    pub fn get_key(&self, peer_id: &PeerId) -> Option<&PublicKey> {
        self.key_book.get(peer_id)
    }
}

/// Store all protocols that the peer supports.
#[derive(Default, Clone)]
pub struct ProtoBook {
    proto_book: HashMap<PeerId, HashMap<String, i32>>,
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
    pub fn add_protocol(&mut self, peer_id: &PeerId, proto: Vec<String>) {
        if let Some(s) = self.proto_book.get_mut(peer_id) {
            for item in proto {
                s.insert(item, 1);
            }
        } else {
            let mut hmap = HashMap::new();
            for item in proto {
                hmap.insert(item, 1);
            }
            self.proto_book.insert(peer_id.clone(), hmap);
        }
    }

    /// Remove support protocol by peer_id
    pub fn remove_protocol(&mut self, peer_id: &PeerId) {
        log::info!("remove protocol");
        self.proto_book.remove(peer_id);
    }

    pub fn get_protocol(&self, peer_id: &PeerId) -> Option<Vec<String>> {
        match self.proto_book.get(peer_id) {
            Some(hmap) => {
                let mut result = Vec::<String>::new();
                for (s, _) in hmap.iter() {
                    result.push(s.parse().unwrap())
                }
                Some(result)
            }
            None => None,
        }
    }

    /// Get the first protocol which matched by given protocols
    pub fn first_supported_protocol(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<String> {
        match self.proto_book.get(peer_id) {
            Some(hmap) => {
                for item in proto {
                    if hmap.contains_key(&item) {
                        return Some(item);
                    }
                }
                None
            }
            None => None,
        }
    }

    /// Search all protocols and return an option that matches by given proto param
    pub fn support_protocol(&self, peer_id: &PeerId, proto: Vec<String>) -> Option<Vec<String>> {
        match self.proto_book.get(peer_id) {
            Some(hmap) => {
                let mut proto_list = Vec::new();
                for item in proto {
                    if hmap.contains_key(&item) {
                        proto_list.push(item)
                    }
                }
                Some(proto_list)
            }
            None => None,
        }
    }

    pub fn get_iter(&self) -> (Vec<PeerId>, Vec<String>) {
        let mut peer = vec![];
        let mut proto = vec![];
        for (k, v) in self.proto_book.iter() {
            peer.push(k.clone());
            for key in v.keys() {
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
    use log::info;
    use std::time::Duration;

    #[test]
    fn addr_book_basic() {
        //env_logger::from_env(env_logger::Env::default().default_filter_or("trace")).init();
        let mut ab = AddrBook::default();

        let peer_id = PeerId::random();

        ab.add_addr(&peer_id, "/memory/123456".parse().unwrap(), Duration::from_secs(1));

        assert_eq!(
            &(ab.get_addr(&peer_id).unwrap().first().unwrap().addr),
            &"/memory/123456".parse().unwrap()
        );

        ab.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1));
        let addrs = ab.get_addr(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.add_addr(&peer_id, "/memory/654321".parse().unwrap(), Duration::from_secs(1));
        let addrs = ab.get_addr(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.update_addr(&peer_id, Duration::from_secs(1), Duration::from_secs(3));
        info!("{}", ab.get_addr(&peer_id).unwrap().first().unwrap().ttl);
        let zero = ab.get_addr(&peer_id).unwrap().first().unwrap().ttl - Duration::from_secs(3).as_secs_f64();
        assert_eq!(zero as i64, 0);

        ab.del_peer(&peer_id);
        assert!(ab.get_addr(&peer_id).is_none());
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
        let support_protocol = proto.support_protocol(&peer_id, option_support_list);
        assert_eq!(
            support_protocol.unwrap(),
            vec!["/libp2p/secio/1.0.0".to_string(), "/libp2p/yamux/1.0.0".to_string()]
        );
    }
}
