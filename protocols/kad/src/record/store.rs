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

mod memory;

pub use memory::{MemoryStore, MemoryStoreConfig};

use super::*;
use crate::{KadError, K_VALUE};
use std::borrow::Cow;
use std::time::Duration;

/// The result of an operation on a `RecordStore`.
pub type Result<T> = std::result::Result<T, KadError>;

/// Trait for types implementing a record store.
///
/// There are two types of records managed by a `RecordStore`:
///
///   1. Regular (value-)records. These records store an arbitrary value
///      associated with a key which is distributed to the closest nodes
///      to the key in the Kademlia DHT as per the standard Kademlia "push-model".
///      These records are subject to re-replication and re-publication as
///      per the standard Kademlia protocol.
///
///   2. Provider records. These records associate the ID of a peer with a key
///      who can supposedly provide the associated value. These records are
///      mere "pointers" to the data which may be followed by contacting these
///      providers to obtain the value. These records are specific to the
///      libp2p Kademlia specification and realise a "pull-model" for distributed
///      content. Just like a regular record, a provider record is distributed
///      to the closest nodes to the key.
///
pub trait RecordStore<'a> {
    type RecordsIter: Iterator<Item = Cow<'a, Record>>;
    type ProviderIter: Iterator<Item = Cow<'a, ProviderRecord>>;

    /// Gets a record from the store, given its key.
    fn get(&'a self, k: &Key) -> Option<Cow<'_, Record>>;

    /// Puts a record into the store.
    fn put(&'a mut self, r: Record) -> Result<()>;

    /// Removes the record with the given key from the store.
    fn remove(&'a mut self, k: &Key);

    /// Gets an iterator over all (value-) records currently stored.
    fn all_records(&'a self) -> Self::RecordsIter;

    /// Gets currently stored records count.
    fn records_count(&'a self) -> usize;

    /// Runs GC for all stored records. As a result, all outdated items
    /// will be removed from storage.
    fn gc_records(&'a mut self, ttl: Duration);

    /// Gets a copy of the stored provider records for the given key.
    fn providers(&'a self, key: &Key) -> Vec<ProviderRecord>;

    /// Adds a provider record to the store.
    ///
    /// A record store only needs to store a number of provider records
    /// for a key corresponding to the replication factor and should
    /// store those records whose providers are closest to the key.
    fn add_provider(&'a mut self, record: ProviderRecord) -> Result<()>;

    /// Removes a provider record from the store.
    fn remove_provider(&'a mut self, k: &Key, p: &PeerId);

    /// Gets an iterator over all stored provider records.
    fn all_providers(&'a self) -> Self::ProviderIter;

    /// Gets currently stored providers count.
    fn providers_count(&'a self) -> usize;

    /// Runs GC for all stored providers. As a result, all outdated items
    /// will be removed from storage.
    fn gc_providers(&'a mut self, ttl: Duration);
}
