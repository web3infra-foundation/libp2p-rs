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

use crate::connection::ConnectionInfo;
use libp2prs_core::PeerId;

/// Information about the network obtained by [`Network::info()`].
#[derive(Debug)]
pub struct NetworkInfo {
    /// The local Peer Id of self node.
    pub id: PeerId,
    /// The total number of connected peers.
    pub num_peers: usize,
    /// The total number of connections.
    pub num_connections: usize,
    /// The total number of pending connections, both incoming and outgoing.
    pub num_connections_pending: usize,
    /// The total number of established connections.
    pub num_connections_established: usize,
    /// The total number of active sub streams.
    pub num_active_streams: usize,
    /// The information of all established connections.
    pub connection_info: Vec<ConnectionInfo>,
}

/// The (optional) configuration for a [`Network`].
///
/// The default configuration specifies no dedicated task executor, no
/// connection limits, a connection event buffer size of 32, and a
/// `notify_handler` buffer size of 8.
#[derive(Default)]
#[allow(dead_code)]
pub struct NetworkConfig {
    /// Note that the `ManagerConfig`s task command buffer always provides
    /// one "free" slot per task. Thus the given total `notify_handler_buffer_size`
    /// exposed for configuration on the `Network` is reduced by one.
    //manager_config: ManagerConfig,
    max_outgoing: Option<usize>,
    max_incoming: Option<usize>,
    max_established_per_peer: Option<usize>,
    max_outgoing_per_peer: Option<usize>,
}
#[allow(dead_code)]
impl NetworkConfig {
    pub fn set_incoming_limit(&mut self, n: usize) -> &mut Self {
        self.max_incoming = Some(n);
        self
    }

    pub fn set_outgoing_limit(&mut self, n: usize) -> &mut Self {
        self.max_outgoing = Some(n);
        self
    }

    pub fn set_established_per_peer_limit(&mut self, n: usize) -> &mut Self {
        self.max_established_per_peer = Some(n);
        self
    }

    pub fn set_outgoing_per_peer_limit(&mut self, n: usize) -> &mut Self {
        self.max_outgoing_per_peer = Some(n);
        self
    }
}
