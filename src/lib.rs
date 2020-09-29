//! ## Summary
//!
//! A multiplexed p2p network framework based on yamux that supports mounting custom protocols.
//!
//! The crate is aimed at implementing a framework that light weight, simple, reliable, high performance, and friendly to users.
//!
//! ### Concept
//!
//! #### Multiaddr
//!
//! [Multiaddr](https://github.com/multiformats/multiaddr) aims to make network addresses future-proof, composable, and efficient.
//!
//! It can express almost all network protocols, such as:
//! - TCP/IP: `/ip4/127.0.0.1/tcp/1337`
//! - DNS/IP: `/dns4/localhost/tcp/1337`
//! - UDP: `/ip4/127.0.0.1/udp/1234`
//!

#![deny(missing_docs)]

/// Re-pub bytes crate
pub use bytes;
/// Traits
pub use libp2p_traits;
/// Re-pub mplex crate
pub use mplex;
/// Re-pub multiaddr crate
pub use multiaddr;
/// Re-pub secio crate
pub use secio;
/// Re-pub yamux crate
pub use yamux;

/*
/// Some gadgets that help create a service
pub mod builder;
/// Context for Session and Service
pub mod context;
/// Error
pub mod error;


mod channel;

pub(crate) mod upnp;
*/
