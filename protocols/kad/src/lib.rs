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

//! Implementation of the libp2p-specific Kademlia protocol.

// TODO: we allow dead_code for now because this library contains a lot of unused code that will
//       be useful later for record store
#![allow(dead_code)]

pub mod kad;
pub mod kbucket;
pub mod protocol;
pub mod record;

mod addresses;
pub mod cli;
mod control;
mod query;
mod task_limit;

pub use control::Control;

mod dht_proto {
    include!(concat!(env!("OUT_DIR"), "/dht.pb.rs"));
}

pub use record::{store, ProviderRecord, Record};

use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;

use futures::channel::{mpsc, oneshot};
use libp2prs_core::PeerId;
use libp2prs_swarm::SwarmError;

/// The `k` parameter of the Kademlia specification.
///
/// This parameter determines:
///
///   1) The (fixed) maximum number of nodes in a bucket.
///   2) The (default) replication factor, which in turn determines:
///       a) The number of closer peers returned in response to a request.
///       b) The number of closest peers to a key to search for in an iterative query.
///
/// The choice of (1) is fixed to this constant. The replication factor is configurable
/// but should generally be no greater than `K_VALUE`. All nodes in a Kademlia
/// DHT should agree on the choices made for (1) and (2).
///
/// The current value is `20`.
pub const K_VALUE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(20) };

/// The `α` parameter of the Kademlia specification.
///
/// This parameter determines the default parallelism for iterative queries,
/// i.e. the allowed number of in-flight requests that an iterative query is
/// waiting for at a particular time while it continues to make progress towards
/// locating the closest peers to a key.
///
/// The current value is `3`.
pub const ALPHA_VALUE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(3) };

/// The `beta` parameter of the Kademlia specification.
///
/// This parameter determines the number of peers closest to a target that must
/// have responded for a query path to terminate.
pub const BETA_VALUE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(3) };

/// The possible errors of Kademlia.
#[derive(Debug)]
pub enum KadError {
    /// The store is at capacity w.r.t. the total number of stored records.
    MaxRecords,
    /// The store is at capacity w.r.t. the total number of stored keys for
    /// provider records.
    MaxProvidedKeys,
    /// The value of a record to be stored is too large.
    ValueTooLarge,
    /// An operation failed to due no known peers in the routing table.
    NoKnownPeers,
    /// An operation is done but nothing found.
    NotFound,
    /// Error while trying to perform the upgrade.
    Upgrade,
    /// Error while bootstrapping Kademlia-DHT.
    Bootstrap,

    /// Internal error, e.g., mpsc::SendError.
    Internal,

    /// Iterative query timeout.
    Timeout,

    /// Error while decoding protobuf.
    Decode,

    /// Received an answer that doesn't correspond to the request.
    UnexpectedMessage(&'static str),
    /// Received a request from an invalid source.
    InvalidSource(PeerId),
    /// Received an request that is not supported yet.
    Unsupported(&'static str),
    /// I/O error in the substream.
    Io(std::io::Error),
    /// Underlying Swarm error.
    Swarm(SwarmError),
    /// Indicates that the Kad main loop is Closing.
    Closing(u32),
}

impl fmt::Display for KadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KadError::MaxRecords => write!(f, "Storage is at capaticy"),
            KadError::NoKnownPeers => write!(f, "No any peers in routing table"),
            KadError::NotFound => write!(f, "Not found"),
            KadError::Timeout => write!(f, "Iterative query timeout"),
            KadError::Swarm(e) => write!(f, "Underlying Swarm error {}", e),
            _ => write!(f, "Kad error"),
        }
    }
}

impl Error for KadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            KadError::MaxRecords => None,
            KadError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<mpsc::SendError> for KadError {
    fn from(_: mpsc::SendError) -> Self {
        KadError::Internal
    }
}

impl From<std::io::Error> for KadError {
    fn from(err: std::io::Error) -> Self {
        KadError::Io(err)
    }
}

impl From<SwarmError> for KadError {
    fn from(err: SwarmError) -> Self {
        KadError::Swarm(err)
    }
}

impl From<oneshot::Canceled> for KadError {
    fn from(_: oneshot::Canceled) -> Self {
        KadError::Internal
    }
}
