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

//! mDNS is a protocol defined by [RFC 6762](https://tools.ietf.org/html/rfc6762) that allows
//! querying nodes that correspond to a certain domain name.
//!
//! In the context of libp2p, the mDNS protocol is used to discover other nodes on the local
//! network that support libp2p.
//!

use libp2prs_core::{Multiaddr, PeerId};

/// Hardcoded name of the mDNS service. Part of the mDNS libp2p specifications.
const SERVICE_NAME: &[u8] = b"_p2p._udp.local";
/// Hardcoded name of the service used for DNS-SD.
const META_QUERY_SERVICE: &[u8] = b"_services._dns-sd._udp.local";

pub mod control;
mod dns;
pub mod service;

use smallvec::alloc::fmt::Formatter;
use std::fmt;

pub struct MdnsConfig {
    /// local Peer ID
    local_peer: PeerId,

    /// List of multiaddresses we're listening on.
    listened_addrs: Vec<Multiaddr>,

    /// Whether we send queries on the network at all.
    /// Note that we still need to have an interval for querying, as we need to wake up the socket
    /// regularly to recover from errors. Otherwise we could simply use an `Option<Interval>`.
    silent: bool,
}

impl MdnsConfig {
    pub fn new(local_peer: PeerId, listened_addrs: Vec<Multiaddr>, silent: bool) -> Self {
        MdnsConfig {
            local_peer,
            listened_addrs,
            silent,
        }
    }
}

#[derive(Clone)]
pub struct AddrInfo {
    pub pid: PeerId,
    pub addrs: Vec<Multiaddr>,
}

impl AddrInfo {
    pub fn new(pid: PeerId, addrs: Vec<Multiaddr>) -> Self {
        AddrInfo { pid, addrs }
    }

    pub fn get_peer(&self) -> &PeerId {
        &self.pid
    }

    pub fn get_addrs(&self) -> &[Multiaddr] {
        &self.addrs
    }
}

impl fmt::Debug for AddrInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "(AddrInfo {} {:?})", self.pid, self.addrs,)
    }
}

impl fmt::Display for AddrInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "(AddrInfo {} {:?})", self.pid, self.addrs,)
    }
}

pub trait Notifee {
    fn handle_peer_found(&mut self, _discovered: AddrInfo) {}
}

pub type INotifiee = Box<dyn Notifee + Send + Sync>;
