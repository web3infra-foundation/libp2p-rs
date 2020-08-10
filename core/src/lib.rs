//! Transports, upgrades, multiplexing and node handling of *libp2p*.
//!
//! The main concepts of libp2p-core are:
//!
//! - A [`PeerId`] is a unique global identifier for a node on the network.
//!   Each node must have a different `PeerId`. Normally, a `PeerId` is the
//!   hash of the public key used to negotiate encryption on the
//!   communication channel, thereby guaranteeing that they cannot be spoofed.

mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}


mod peer_id;
pub mod identity;

pub use peer_id::PeerId;
pub use identity::PublicKey;

