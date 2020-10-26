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

//! Transports, upgrades, multiplexing and node handling of *libp2p*.
//!
//! The main concepts of libp2p-core are:
//!
//! - A [`PeerId`] is a unique global identifier for a node on the network.
//!   Each node must have a different `PeerId`. Normally, a `PeerId` is the
//!   hash of the public key used to negotiate encryption on the
//!   communication channel, thereby guaranteeing that they cannot be spoofed.
//! - The [`Transport`] trait defines how to reach a remote node or listen for
//!   incoming remote connections. See the `transport` module.
//! - The [`StreamMuxer`] trait is implemented on structs that hold a connection
//!   to a remote and can subdivide this connection into multiple substreams.
//!   See the `muxing` module.
//! - The [`UpgradeInfo`] and [`Upgrader`] traits define how to upgrade each
//!   individual substream to use a protocol.
//!   See the `upgrade` module.

pub mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

// re-export multiaddr
pub use libp2prs_multiaddr as multiaddr;

pub mod identity;
mod peer_id;

pub mod multistream;

pub use identity::PublicKey;
pub use peer_id::PeerId;

pub mod transport;
pub use libp2prs_multiaddr::Multiaddr;
pub use transport::Transport;

pub mod muxing;
pub mod secure_io;
pub mod upgrade;

pub mod either;

pub mod peerstore;

pub mod pnet;
