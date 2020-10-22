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
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, SystemTime};

#[derive(Default, Clone)]
pub struct PeerStore {
    pub addrs: AddrBook,
    pub protos: ProtoBook,
    pub keys: KeyBook,
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
#[derive(Clone, Debug)]
pub struct AddrBookRecord {
    pub addr: Multiaddr,
    ttl: f64,
    expiry: Duration,
}

impl AddrBookRecord {
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl.as_secs_f64()
    }

    pub fn set_expiry(&mut self, expiry: Duration) {
        self.expiry = expiry
    }

    pub fn get_ttl(&self) -> f64 {
        self.ttl
    }

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
    pub fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration) {
        if let Some(entry) = self.addr_book.get_mut(peer_id) {
            let mut exist = false;
            for (count, i) in entry.iter().enumerate() {
                if i.addr == addr {
                    // In order to get mutable
                    let record: &mut AddrBookRecord = entry.get_mut(count).unwrap();
                    let enpiry = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .checked_add(ttl)
                        .unwrap();
                    record.set_expiry(enpiry);
                    exist = true;
                    break;
                }
            }
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
        let mut count = 0;
        let iter = addr.clone();
        for item in iter.into_iter() {
            if item.expiry > time {
                count += 1;
                continue;
            } else {
                addr.remove(count);
                count += 1;
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
    pub fn add_key(&mut self, peer_id: &PeerId, key: PublicKey) {
        if self.key_book.get(peer_id).is_none() {
            self.key_book.insert(peer_id.clone(), key);
        }
    }

    pub fn del_key(&mut self, peer_id: &PeerId) {
        self.key_book.remove(peer_id);
    }

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

    // Get the first protocol which matched by given protocols
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

    // Search all protocols and return an option that matches by given proto param
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
}

#[cfg(test)]
mod tests {
    use crate::peerstore::{AddrBook, ProtoBook};
    use crate::PeerId;
    use log::info;
    use std::time::Duration;

    #[test]
    #[allow(clippy::float_cmp)]
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
        assert_eq!(
            ab.get_addr(&peer_id).unwrap().first().unwrap().ttl,
            Duration::from_secs(3).as_secs_f64()
        );

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
