/// Information about the network obtained by [`Network::info()`].
#[derive(Clone, Debug)]
pub struct NetworkInfo {
    /// The total number of connected peers.
    pub num_peers: usize,
    /// The total number of connections, both established and pending.
    pub num_connections: usize,
    /// The total number of pending connections, both incoming and outgoing.
    pub num_connections_pending: usize,
    /// The total number of established connections.
    pub num_connections_established: usize,
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
