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

//! The `Entry` API for quering and modifying the entries of a `KBucketsTable`
//! representing the nodes participating in the Kademlia DHT.

pub use super::bucket::{Node, K_VALUE};
pub use super::key::*;

use super::*;

/// An immutable by-reference view of a bucket entry.
pub struct EntryRefView<'a, TPeerId, TVal> {
    /// The node represented by the entry.
    pub node: NodeRefView<'a, TPeerId, TVal>,
}

/// An immutable by-reference view of a `Node`.
pub struct NodeRefView<'a, TKey, TVal> {
    pub key: &'a TKey,
    pub value: &'a TVal,
}

impl<TKey, TVal> EntryRefView<'_, TKey, TVal> {
    pub fn to_owned(&self) -> EntryView<TKey, TVal>
    where
        TKey: Clone,
        TVal: Clone,
    {
        EntryView {
            node: Node {
                key: self.node.key.clone(),
                value: self.node.value.clone(),
            },
        }
    }
}

/// A cloned, immutable view of an entry that is present in a bucket.
#[derive(Clone, Debug)]
pub struct EntryView<TKey, TVal> {
    /// The node represented by the entry.
    pub node: Node<TKey, TVal>,
}

impl<TKey: AsRef<KeyBytes>, TVal> AsRef<KeyBytes> for EntryView<TKey, TVal> {
    fn as_ref(&self) -> &KeyBytes {
        self.node.key.as_ref()
    }
}

/// A reference into a single entry of a `KBucketsTable`.
#[derive(Debug)]
pub enum Entry<'a, TPeerId, TVal> {
    /// The entry is present in a bucket.
    Present(PresentEntry<'a, TPeerId, TVal>),
    /// The entry is absent and may be inserted.
    Absent(AbsentEntry<'a, TPeerId, TVal>),
    /// The entry represents the local node.
    SelfEntry,
}

/// The internal representation of the different states of an `Entry`,
/// referencing the associated key and bucket.
#[derive(Debug)]
struct EntryRef<'a, TKey, TVal> {
    bucket: &'a mut KBucket<TKey, TVal>,
    key: &'a TKey,
}

impl<'a, TKey, TVal> Entry<'a, TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    /// Creates a new `Entry` for a `Key`, encapsulating access to a bucket.
    pub(super) fn new(bucket: &'a mut KBucket<TKey, TVal>, key: &'a TKey) -> Self {
        if bucket.position(key).is_some() {
            Entry::Present(PresentEntry::new(bucket, key))
        } else {
            Entry::Absent(AbsentEntry::new(bucket, key))
        }
    }

    /// Creates an immutable by-reference view of the entry.
    ///
    /// Returns `None` if the entry isn't present in a bucket.
    pub fn view(&'a mut self) -> Option<EntryRefView<'a, TKey, TVal>> {
        match self {
            Entry::Present(entry) => Some(EntryRefView {
                node: NodeRefView {
                    key: entry.0.key,
                    value: entry.value(),
                },
            }),
            _ => None,
        }
    }

    /// Returns the key of the entry.
    ///
    /// Returns `None` if the `Key` used to construct this `Entry` is not a valid
    /// key for an entry in a bucket, which is the case for the `local_key` of
    /// the `KBucketsTable` referring to the local node.
    pub fn key(&self) -> Option<&TKey> {
        match self {
            Entry::Present(entry) => Some(entry.key()),
            Entry::Absent(entry) => Some(entry.key()),
            Entry::SelfEntry => None,
        }
    }

    /// Returns the value associated with the entry.
    ///
    /// Returns `None` if the entry is absent from any bucket or refers to the
    /// local node.
    pub fn value(&mut self) -> Option<&mut TVal> {
        match self {
            Entry::Present(entry) => Some(entry.value()),
            Entry::Absent(_) => None,
            Entry::SelfEntry => None,
        }
    }
}

/// An entry present in a bucket.
#[derive(Debug)]
pub struct PresentEntry<'a, TKey, TVal>(EntryRef<'a, TKey, TVal>);

impl<'a, TKey, TVal> PresentEntry<'a, TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    fn new(bucket: &'a mut KBucket<TKey, TVal>, key: &'a TKey) -> Self {
        PresentEntry(EntryRef { bucket, key })
    }

    /// Returns the key of the entry.
    pub fn key(&self) -> &TKey {
        self.0.key
    }

    /// Returns the value associated with the key.
    pub fn value(&mut self) -> &mut TVal {
        &mut self
            .0
            .bucket
            .get_mut(self.0.key)
            .expect("We can only build a PresentEntry if the entry is in the bucket; QED")
            .value
    }

    /// Removes the entry from the bucket.
    pub fn remove(self) -> EntryView<TKey, TVal> {
        let (node, _pos) = self
            .0
            .bucket
            .remove(self.0.key)
            .expect("We can only build a PresentEntry if the entry is in the bucket; QED");
        EntryView { node }
    }
}

/// An entry that is not present in any bucket.
#[derive(Debug)]
pub struct AbsentEntry<'a, TKey, TVal>(EntryRef<'a, TKey, TVal>);

impl<'a, TKey, TVal> AbsentEntry<'a, TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    fn new(bucket: &'a mut KBucket<TKey, TVal>, key: &'a TKey) -> Self {
        AbsentEntry(EntryRef { bucket, key })
    }

    /// Returns the key of the entry.
    pub fn key(&self) -> &TKey {
        self.0.key
    }

    /// Returns the reference of the KBucket.
    pub fn bucket(&mut self) -> &mut KBucket<TKey, TVal> {
        self.0.bucket
    }

    /// Attempts to insert the entry into a bucket.
    pub fn insert(&mut self, value: TVal) -> bool {
        self.0.bucket.insert(Node {
            key: self.0.key.clone(),
            value,
        })
    }
}
