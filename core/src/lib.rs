//! Transports, upgrades, multiplexing and node handling of *libp2p*.
//!
//! The main concepts of libp2p-core are:
//!
//! - A [`PeerId`] is a unique global identifier for a node on the network.
//!   Each node must have a different `PeerId`. Normally, a `PeerId` is the
//!   hash of the public key used to negotiate encryption on the
//!   communication channel, thereby guaranteeing that they cannot be spoofed.


pub mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

// re-export multiaddr
pub use multiaddr;


pub mod identity;
mod peer_id;

pub use identity::PublicKey;
pub use peer_id::PeerId;

pub mod transport;
pub use transport::Transport;
pub use multiaddr::Multiaddr;

pub mod upgrade;
pub mod muxing;
pub mod either;