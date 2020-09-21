use crate::{Multiaddr, PeerId};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

#[derive(Default)]
pub struct PeerStore {
    pub addrs: AddrBook,
}

#[derive(Default)]
pub struct AddrBook {
    pub book: HashMap<PeerId, SmallVec<[Multiaddr; 4]>>,
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

impl fmt::Debug for AddrBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AddrBook").field(&self.book).finish()
    }
}

impl fmt::Display for AddrBook {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        //self.book.iter().for_each(|a| a.0.fmt(f)
        Ok(())
    }
}

impl AddrBook {
    pub fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, _ttl: Duration) {
        if let Some(entry) = self.book.get_mut(peer_id) {
            if !entry.contains(&addr) {
                entry.push(addr);
            }
        } else {
            let vec = vec![addr];
            self.book.insert(peer_id.clone(), SmallVec::from_vec(vec));
        }
    }
    pub fn del_peer(&mut self, peer_id: &PeerId) {
        self.book.remove(peer_id);
    }
    pub fn get_addr(&self, peer_id: &PeerId) -> Option<&SmallVec<[Multiaddr; 4]>> {
        self.book.get(peer_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::peerstore::AddrBook;
    use crate::PeerId;
    use std::time::Duration;

    #[test]
    fn addr_book_basic() {
        let mut ab = AddrBook::default();

        let peer_id = PeerId::random();

        ab.add_addr(
            &peer_id,
            "/memory/123456".parse().unwrap(),
            Duration::from_secs(1),
        );

        assert_eq!(
            ab.get_addr(&peer_id).unwrap().first(),
            Some(&"/memory/123456".parse().unwrap())
        );

        ab.add_addr(
            &peer_id,
            "/memory/654321".parse().unwrap(),
            Duration::from_secs(1),
        );
        let addrs = ab.get_addr(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.add_addr(
            &peer_id,
            "/memory/654321".parse().unwrap(),
            Duration::from_secs(1),
        );
        let addrs = ab.get_addr(&peer_id).unwrap();
        assert_eq!(addrs.len(), 2);

        ab.del_peer(&peer_id);
        assert!(ab.get_addr(&peer_id).is_none());
    }
}
