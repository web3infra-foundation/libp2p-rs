// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! The internal API for a single `KBucket` in a `KBucketsTable`.
//!
//! > **Note**: Uniqueness of entries w.r.t. a `Key` in a `KBucket` is not
//! > checked in this module. This is an invariant that must hold across all
//! > buckets in a `KBucketsTable` and hence is enforced by the public API
//! > of the `KBucketsTable` and in particular the public `Entry` API.

use super::*;
pub use crate::K_VALUE;

//
// /// The status of a node in a bucket.
// ///
// /// The status of a node in a bucket together with the time of the
// /// last status change determines the position of the node in a
// /// bucket.
// #[derive(PartialEq, Eq, Debug, Copy, Clone)]
// pub enum NodeStatus {
//     /// The node is considered connected.
//     Connected,
//     /// The node is considered disconnected.
//     Disconnected
// }

/// A `Node` in a bucket, representing a peer participating
/// in the Kademlia DHT together with an associated value (e.g. contact
/// information).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<TKey, TVal> {
    /// The key of the node, identifying the peer.
    pub key: TKey,
    /// The associated value.
    pub value: TVal,
}

/// The position of a node in a `KBucket`, i.e. a non-negative integer
/// in the range `[0, K_VALUE)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position(usize);

/// A `KBucket` is a list of up to `K_VALUE` keys and associated values,
/// ordered from least-recently connected to most-recently connected.
#[derive(Debug, Clone)]
pub struct KBucket<TKey, TVal> {
    /// The nodes contained in the bucket.
    nodes: ArrayVec<[Node<TKey, TVal>; K_VALUE.get()]>,
}

impl<TKey, TVal> Default for KBucket<TKey, TVal> {
    fn default() -> Self {
        KBucket { nodes: ArrayVec::new() }
    }
}

impl<TKey, TVal> KBucket<TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    /// Creates a new `KBucket` with the given timeout for pending entries.
    pub fn new() -> Self {
        KBucket { nodes: ArrayVec::new() }
    }

    /// Returns a reference to a node in the bucket.
    pub fn get(&self, key: &TKey) -> Option<&Node<TKey, TVal>> {
        self.position(key).map(|p| &self.nodes[p.0])
    }

    /// Returns an iterator over the nodes in the bucket, together with their status.
    pub fn iter(&self) -> impl Iterator<Item = &Node<TKey, TVal>> {
        self.nodes.iter()
    }

    /// Inserts a new node into the bucket.
    pub fn insert(&mut self, node: Node<TKey, TVal>) -> bool {
        if self.nodes.is_full() {
            false
        } else {
            self.nodes.push(node);
            true
        }
    }

    /// Removes the node with the given key from the bucket, if it exists.
    pub fn remove(&mut self, key: &TKey) -> Option<(Node<TKey, TVal>, Position)> {
        if let Some(pos) = self.position(key) {
            // Remove the node from its current position.
            let node = self.nodes.remove(pos.0);
            Some((node, pos))
        } else {
            None
        }
    }

    /// Gets the number of entries currently in the bucket.
    pub fn num_entries(&self) -> usize {
        self.nodes.len()
    }

    /// Gets the position of a node in the bucket.
    pub fn position(&self, key: &TKey) -> Option<Position> {
        self.nodes.iter().position(|p| p.key.as_ref() == key.as_ref()).map(Position)
    }

    /// Gets a mutable reference to the node identified by the given key.
    ///
    /// Returns `None` if the given key does not refer to a node in the
    /// bucket.
    pub fn get_mut(&mut self, key: &TKey) -> Option<&mut Node<TKey, TVal>> {
        self.nodes.iter_mut().find(move |p| p.key.as_ref() == key.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2prs_core::PeerId;
    use quickcheck::*;
    use rand::Rng;

    impl Arbitrary for KBucket<Key<PeerId>, ()> {
        fn arbitrary<G: Gen>(g: &mut G) -> KBucket<Key<PeerId>, ()> {
            let mut bucket = KBucket::<Key<PeerId>, ()>::default();
            let num_nodes = g.gen_range(1, K_VALUE.get() + 1);
            for _ in 0..num_nodes {
                let key = Key::from(PeerId::random());
                let node = Node {
                    key: key.clone(),
                    value: (),
                };
                if !bucket.insert(node) {
                    panic!()
                }
            }
            bucket
        }
    }

    impl Arbitrary for Position {
        fn arbitrary<G: Gen>(g: &mut G) -> Position {
            Position(g.gen_range(0, K_VALUE.get()))
        }
    }

    // Fill a bucket with random nodes.
    fn fill_bucket(bucket: &mut KBucket<Key<PeerId>, ()>) {
        let num_entries_start = bucket.num_entries();
        for i in 0..K_VALUE.get() - num_entries_start {
            let key = Key::from(PeerId::random());
            let node = Node { key, value: () };
            assert!(bucket.insert(node));
            assert_eq!(bucket.num_entries(), num_entries_start + i + 1);
        }
    }

    #[test]
    fn full_bucket() {
        let mut bucket = KBucket::<Key<PeerId>, ()>::default();

        // Fill the bucket.
        fill_bucket(&mut bucket);

        // Trying to insert another disconnected node fails.
        let key = Key::from(PeerId::random());
        let node = Node { key, value: () };
        if bucket.insert(node) {
            panic!("")
        }

        assert_eq!(K_VALUE.get(), bucket.num_entries());
    }

    #[test]
    fn remove_bucket() {
        let mut bucket = KBucket::<Key<PeerId>, ()>::default();

        let key = Key::from(PeerId::random());
        let node = Node {
            key: key.clone(),
            value: (),
        };
        bucket.insert(node.clone());

        fill_bucket(&mut bucket);

        let (value, pos) = bucket.remove(&key).unwrap();
        assert_eq!(node, value);
        assert_eq!(Position(0), pos);
    }
}
