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

//! Libp2p-rs is a peer-to-peer framework.
//!
//! ## Summary
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

// re-export xCLI
pub use xcli;

#[doc(inline)]
pub use libp2prs_core as core;
#[cfg(any(feature = "dns-async-std", feature = "dns-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "dns-async-std", feature = "dns-tokio"))))]
#[doc(inline)]
pub use libp2prs_dns as dns;
#[cfg(feature = "exporter")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "exporter-async-std", feature = "exporter-tokio"))))]
#[doc(inline)]
pub use libp2prs_exporter as exporter;
#[cfg(any(feature = "floodsub-async-std", feature = "floodsub-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "floodsub-async-std", feature = "floodsub-tokio"))))]
#[doc(inline)]
pub use libp2prs_floodsub as floodsub;
#[cfg(any(feature = "gossipsub-async-std", feature = "gossipsub-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "gossipsub-async-std", feature = "gossipsub-tokio"))))]
#[doc(inline)]
pub use libp2prs_gossipsub as gossipsub;
#[cfg(feature = "infoserver")]
#[cfg_attr(docsrs, doc(cfg(any(feature = "infoserver-async-std", feature = "infoserver-tokio"))))]
#[doc(inline)]
pub use libp2prs_infoserver as infoserver;
#[cfg(any(feature = "kad-async-std", feature = "kad-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "kad-async-std", feature = "kad-tokio"))))]
#[doc(inline)]
pub use libp2prs_kad as kad;
#[cfg(any(feature = "mdns-async-std", feature = "mdns-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "mdns-async-std", feature = "mdns-tokio"))))]
#[doc(inline)]
pub use libp2prs_mdns as mdns;
#[cfg(feature = "mplex")]
#[cfg_attr(docsrs, doc(cfg(feature = "mplex")))]
#[doc(inline)]
pub use libp2prs_mplex as mplex;
#[doc(inline)]
pub use libp2prs_multiaddr as multiaddr;
#[cfg(feature = "noise")]
#[cfg_attr(docsrs, doc(cfg(feature = "noise")))]
#[doc(inline)]
pub use libp2prs_noise as noise;
#[cfg(feature = "plaintext")]
#[cfg_attr(docsrs, doc(cfg(feature = "plaintext")))]
#[doc(inline)]
pub use libp2prs_plaintext as plaintext;
#[cfg(any(feature = "rt-async-std", feature = "rt-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "rt-async-std", feature = "rt-tokio"))))]
#[doc(inline)]
pub use libp2prs_runtime as runtime;
#[cfg(feature = "secio")]
#[cfg_attr(docsrs, doc(cfg(feature = "secio")))]
#[doc(inline)]
pub use libp2prs_secio as secio;
#[doc(inline)]
pub use libp2prs_swarm as swarm;
#[cfg(any(feature = "tcp-async-std", feature = "tcp-tokio"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "tcp-async-std", feature = "tcp-tokio"))))]
#[doc(inline)]
pub use libp2prs_tcp as tcp;
#[cfg(feature = "websocket")]
#[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
#[doc(inline)]
pub use libp2prs_websocket as websocket;
#[cfg(feature = "yamux")]
#[cfg_attr(docsrs, doc(cfg(feature = "yamux")))]
#[doc(inline)]
pub use libp2prs_yamux as yamux;
