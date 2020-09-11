use std::fmt;
use std::error::Error;
use libp2p_core::{Multiaddr, PeerId};
use std::hash::Hash;

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

impl NetworkConfig {
/*    pub fn set_executor(&mut self, e: Box<dyn Executor + Send>) -> &mut Self {
        self.manager_config.executor = Some(e);
        self
    }

    /// Shortcut for calling `executor` with an object that calls the given closure.
    pub fn set_executor_fn(mut self, f: impl Fn(Pin<Box<dyn Future<Output = ()> + Send>>) + Send + 'static) -> Self {
        struct SpawnImpl<F>(F);
        impl<F: Fn(Pin<Box<dyn Future<Output = ()> + Send>>)> Executor for SpawnImpl<F> {
            fn exec(&self, f: Pin<Box<dyn Future<Output = ()> + Send>>) {
                (self.0)(f)
            }
        }
        self.set_executor(Box::new(SpawnImpl(f)));
        self
    }

    pub fn executor(&self) -> Option<&Box<dyn Executor + Send>> {
        self.manager_config.executor.as_ref()
    }

    /// Sets the maximum number of events sent to a connection's background task
    /// that may be buffered, if the task cannot keep up with their consumption and
    /// delivery to the connection handler.
    ///
    /// When the buffer for a particular connection is full, `notify_handler` will no
    /// longer be able to deliver events to the associated `ConnectionHandler`,
    /// thus exerting back-pressure on the connection and peer API.
    pub fn set_notify_handler_buffer_size(&mut self, n: NonZeroUsize) -> &mut Self {
        self.manager_config.task_command_buffer_size = n.get() - 1;
        self
    }

    /// Sets the maximum number of buffered connection events (beyond a guaranteed
    /// buffer of 1 event per connection).
    ///
    /// When the buffer is full, the background tasks of all connections will stall.
    /// In this way, the consumers of network events exert back-pressure on
    /// the network connection I/O.
    pub fn set_connection_event_buffer_size(&mut self, n: usize) -> &mut Self {
        self.manager_config.task_event_buffer_size = n;
        self
    }
*/

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
