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

//! Records and record storage abstraction of the libp2p Kademlia DHT.

pub mod store;

use bytes::Bytes;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use libp2prs_core::{multihash::Multihash, PeerId};

/// The (opaque) key of a record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key(Bytes);

impl Key {
    /// Creates a new key from the bytes of the input.
    pub fn new<K: AsRef<[u8]>>(key: &K) -> Self {
        Key(Bytes::copy_from_slice(key.as_ref()))
    }

    /// Copies the bytes of the key into a new vector.
    pub fn to_vec(&self) -> Vec<u8> {
        Vec::from(&self.0[..])
    }
}

impl Borrow<[u8]> for Key {
    fn borrow(&self) -> &[u8] {
        &self.0[..]
    }
}

impl AsRef<[u8]> for Key {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<Vec<u8>> for Key {
    fn from(v: Vec<u8>) -> Key {
        Key(Bytes::from(v))
    }
}

impl From<Multihash> for Key {
    fn from(m: Multihash) -> Key {
        Key::from(m.to_bytes())
    }
}

impl From<PeerId> for Key {
    fn from(m: PeerId) -> Key {
        Key::from(m.to_bytes())
    }
}

/// A record stored in the DHT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Key of the record.
    pub key: Key,
    /// Value of the record.
    pub value: Vec<u8>,
    /// The timestamp when the record is received from Kademlia network.
    /// `None` means record is created locally.
    pub time_received: Option<Instant>,
    /// The (original) publisher of the record.
    pub publisher: Option<PeerId>,
}

impl Record {
    /// Creates a new record for insertion into the DHT.
    pub fn new<K>(key: K, value: Vec<u8>, local: bool, publisher: Option<PeerId>) -> Self
    where
        K: Into<Key>,
    {
        Record {
            key: key.into(),
            value,
            time_received: if local { None } else { Some(Instant::now()) },
            publisher,
        }
    }

    /// Returns the timestamp of the record, `Instant`.
    pub fn timestamp(&self) -> Option<Instant> {
        self.time_received
    }
}

/// A record stored in the DHT whose value is the ID of a peer
/// who can provide the value on-demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRecord {
    /// The key whose value is provided by the provider.
    pub key: Key,
    /// The provider of the value for the key.
    pub provider: PeerId,
    /// The timestamp when the provider record is received from Kademlia network.
    /// `None` means it is created locally.
    pub time_received: Option<Instant>,
}

// Skip clippy::derive_hash_xor_eq, but it may to be resolved.
#[allow(clippy::derive_hash_xor_eq)]
impl Hash for ProviderRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.provider.hash(state);
    }
}

impl ProviderRecord {
    /// Creates a new provider record for insertion into a `RecordStore`.
    pub fn new<K>(key: K, provider: PeerId, local: bool) -> Self
    where
        K: Into<Key>,
    {
        ProviderRecord {
            key: key.into(),
            provider,
            time_received: if local { None } else { Some(Instant::now()) },
        }
    }

    /// Returns the timestamp of the provider, `Instant`.
    pub fn timestamp(&self) -> Option<Instant> {
        self.time_received
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2prs_core::multihash::{Code, Multihash};
    use quickcheck::*;
    use rand::Rng;
    use std::time::Duration;

    impl Arbitrary for Key {
        fn arbitrary<G: Gen>(_: &mut G) -> Key {
            let hash = rand::thread_rng().gen::<[u8; 32]>();
            Key::from(Multihash::wrap(Code::Sha2_256.into(), &hash).unwrap())
        }
    }

    impl Arbitrary for Record {
        fn arbitrary<G: Gen>(g: &mut G) -> Record {
            Record {
                key: Key::arbitrary(g),
                value: Vec::arbitrary(g),
                time_received: if g.gen() {
                    Some(Instant::now() + Duration::from_secs(g.gen_range(0, 60)))
                } else {
                    None
                },
                publisher: if g.gen() { Some(PeerId::random()) } else { None },
            }
        }
    }

    impl Arbitrary for ProviderRecord {
        fn arbitrary<G: Gen>(g: &mut G) -> ProviderRecord {
            ProviderRecord {
                key: Key::arbitrary(g),
                provider: PeerId::random(),
                time_received: Some(Instant::now() + Duration::from_secs(g.gen_range(0, 60))),
            }
        }
    }
}
