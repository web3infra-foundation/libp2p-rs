//! High level manager of the network.
//!
//! A [`Swarm`] contains the state of the network as a whole. The entire
//! behaviour of a libp2p network can be controlled through the `Swarm`.
//! The `Swarm` struct contains all active and pending connections to
//! remotes and manages the state of all the substreams that have been
//! opened, and all the upgrades that were built upon these substreams.
//!
//! # Initializing a Swarm
//!
//! Creating a `Swarm` requires three things:
//!
//!  1. A network identity of the local node in form of a [`PeerId`].
//!  2. An implementation of the [`Transport`] trait. This is the type that
//!     will be used in order to reach nodes on the network based on their
//!     address. See the `transport` module for more information.
//!  3. An implementation of the [`NetworkBehaviour`] trait. This is a state
//!     machine that defines how the swarm should behave once it is connected
//!     to a node.
//!
//! # Network Behaviour
//!
//! The [`NetworkBehaviour`] trait is implemented on types that indicate to
//! the swarm how it should behave. This includes which protocols are supported
//! and which nodes to try to connect to. It is the `NetworkBehaviour` that
//! controls what happens on the network. Multiple types that implement
//! `NetworkBehaviour` can be composed into a single behaviour.
//!
//! # Protocols Handler
//!
//! The [`ProtocolsHandler`] trait defines how each active connection to a
//! remote should behave: how to handle incoming substreams, which protocols
//! are supported, when to open a new outbound substream, etc.
//!

//mod behaviour;
mod connection;
mod network;
mod registry;

use crate::connection::{ConnectedPoint, Connection, ConnectionId, ConnectionLimit, Direction};
use crate::network::{NetworkConfig, NetworkInfo};

use libp2p_core::peerstore::PeerStore;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::TransportListener;
use libp2p_core::{muxing::StreamMuxer, transport::TransportError, Multiaddr, PeerId, Transport};
use registry::Addresses;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::{error, fmt, hash::Hash, io};

/// The ID of a single listener.
///
/// It is part of most [`ListenersEvent`]s and can be used to remove
/// individual listeners from the [`ListenersStream`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListenerId(u64);

/// A single active listener.
#[derive(Debug)]
struct Listener<TTrans>
where
    TTrans: Transport,
{
    /// The ID of this listener.
    id: ListenerId,
    /// The object that actually listens.
    listener: TTrans::Listener,
}

#[allow(dead_code)]
impl<TTrans> Listener<TTrans>
where
    TTrans: Transport,
{
    pub fn new(listener: TTrans::Listener, id: ListenerId) -> Self {
        Listener { id, listener }
    }
}

/// Event generated by the `Swarm`.
#[derive(Debug)]
pub enum SwarmEvent<TBvEv /*, THandleErr*/> {
    /// Event generated by the `NetworkBehaviour`.
    Behaviour(TBvEv),
    /// A connection to the given peer has been opened.
    ConnectionEstablished {
        /// Identity of the peer that we have connected to.
        peer_id: PeerId,
        /// Endpoint of the connection that has been opened.
        endpoint: ConnectedPoint,
        /// Number of established connections to this peer, including the one that has just been
        /// opened.
        num_established: NonZeroU32,
    },
    /// A connection with the given peer has been closed.
    ConnectionClosed {
        /// Identity of the peer that we have connected to.
        peer_id: PeerId,
        /// Endpoint of the connection that has been closed.
        endpoint: ConnectedPoint,
        /// Number of other remaining connections to this same peer.
        num_established: u32,
        // /// Reason for the disconnection.
        // cause: ConnectionError<NodeHandlerWrapperError<THandleErr>>,
    },
    /// A new connection arrived on a listener and is in the process of protocol negotiation.
    ///
    /// A corresponding [`ConnectionEstablished`](SwarmEvent::ConnectionEstablished),
    /// [`BannedPeer`](SwarmEvent::BannedPeer), or
    /// [`IncomingConnectionError`](SwarmEvent::IncomingConnectionError) event will later be
    /// generated for this connection.
    IncomingConnection {
        /// Local connection address.
        /// This address has been earlier reported with a [`NewListenAddr`](SwarmEvent::NewListenAddr)
        /// event.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
    },
    /// An error happened on a connection during its initial handshake.
    ///
    /// This can include, for example, an error during the handshake of the encryption layer, or
    /// the connection unexpectedly closed.
    IncomingConnectionError {
        /// Local connection address.
        /// This address has been earlier reported with a [`NewListenAddr`](SwarmEvent::NewListenAddr)
        /// event.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        // /// The error that happened.
        // error: PendingConnectionError<io::Error>,
    },
    /// We connected to a peer, but we immediately closed the connection because that peer is banned.
    BannedPeer {
        /// Identity of the banned peer.
        peer_id: PeerId,
        /// Endpoint of the connection that has been closed.
        endpoint: ConnectedPoint,
    },
    /// Tried to dial an address but it ended up being unreachaable.
    UnreachableAddr {
        /// `PeerId` that we were trying to reach.
        peer_id: PeerId,
        /// Address that we failed to reach.
        address: Multiaddr,
        // /// Error that has been encountered.
        // error: PendingConnectionError<io::Error>,
        /// Number of remaining connection attempts that are being tried for this peer.
        attempts_remaining: u32,
    },
    /// Tried to dial an address but it ended up being unreachaable.
    /// Contrary to `UnreachableAddr`, we don't know the identity of the peer that we were trying
    /// to reach.
    UnknownPeerUnreachableAddr {
        /// Address that we failed to reach.
        address: Multiaddr,
        // /// Error that has been encountered.
        // error: PendingConnectionError<io::Error>,
    },
    /// One of our listeners has reported a new local listening address.
    NewListenAddr(Multiaddr),
    /// One of our listeners has reported the expiration of a listening address.
    ExpiredListenAddr(Multiaddr),
    /// One of the listeners gracefully closed.
    ListenerClosed {
        /// The addresses that the listener was listening on. These addresses are now considered
        /// expired, similar to if a [`ExpiredListenAddr`](SwarmEvent::ExpiredListenAddr) event
        /// has been generated for each of them.
        addresses: Vec<Multiaddr>,
        /// Reason for the closure. Contains `Ok(())` if the stream produced `None`, or `Err`
        /// if the stream produced an error.
        reason: Result<(), io::Error>,
    },
    /// One of the listeners reported a non-fatal error.
    ListenerError {
        /// The listener error.
        error: io::Error,
    },
    /// A new dialing attempt has been initiated.
    ///
    /// A [`ConnectionEstablished`](SwarmEvent::ConnectionEstablished)
    /// event is reported if the dialing attempt succeeds, otherwise a
    /// [`UnreachableAddr`](SwarmEvent::UnreachableAddr) event is reported
    /// with `attempts_remaining` equal to 0.
    Dialing(PeerId),
}

/// Contains the state of the network, plus the way it should behave.
pub struct Swarm<TTrans, THandler>
where
    TTrans: Transport + Clone,
    TTrans::Output: StreamMuxer,
    //    THandler: IntoProtocolsHandler,
    //    TConnInfo: ConnectionInfo<PeerId = PeerId>,
{
    peers: PeerStore,

    transport: TTrans,
    #[allow(dead_code)]
    handler: THandler,
    /// The local peer ID.
    local_peer_id: PeerId,

    /// Listeners for incoming connections.
    listeners: SmallVec<[Listener<TTrans>; 10]>,
    /// The next listener ID to assign.
    next_id: ListenerId,

    /*
        /// Handles which nodes to connect to and how to handle the events sent back by the protocol
        /// handlers.
        behaviour: TBehaviour,

    */
    /// List of protocols that the behaviour says it supports.
    #[allow(dead_code)]
    supported_protocols: SmallVec<[Vec<u8>; 16]>,

    /// List of multiaddresses we're listening on.
    listened_addrs: SmallVec<[Multiaddr; 8]>,

    /// List of multiaddresses we're listening on, after account for external IP addresses and
    /// similar mechanisms.
    external_addrs: Addresses,

    /// List of nodes for which we deny any incoming connection.
    banned_peers: HashSet<PeerId>,

    /// The active connections, both incoming and outgoing
    connections: HashMap<PeerId, Connection<TTrans::Output>>,
    //
    // /// Pending event to be delivered to connection handlers
    // /// (or dropped if the peer disconnected) before the `behaviour`
    // /// can be polled again.
    // pending_event: Option<(PeerId, PendingNotifyHandler, TInEvent)>
}

// impl<TBehaviour, TInEvent, TOutEvent, THandler, TConnInfo> Unpin for
//     Swarm<TBehaviour, TInEvent, TOutEvent, THandler, TConnInfo>
// where
//     THandler: IntoProtocolsHandler,
//     TConnInfo: ConnectionInfo<PeerId = PeerId>,
// {
// }

impl<TTrans, THandler> Swarm<TTrans, THandler>
where
    TTrans: Transport + Clone,
    TTrans::Output: StreamMuxer + SecureInfo,
    // TInEvent: Clone + Send + 'static,
    // TOutEvent: Send + 'static,
    // TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
    //THandler: IntoProtocolsHandler + Send + 'static,
    //THandler::Handler: ProtocolsHandler<InEvent = TInEvent, OutEvent = TOutEvent, Error = THandleErr>,
    //THandleErr: error::Error + Send + 'static,
{
    /// Builds a new `Swarm`.
    pub fn new<TMuxer>(
        transport: TTrans,
        handler: THandler,
        local_peer_id: PeerId,
        _config: NetworkConfig,
    ) -> Self
// where
    //     TMuxer: StreamMuxer + Send + Sync + 'static,
    //     TMuxer::OutboundSubstream: Send + 'static,
    //     <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
    //     <TMuxer as StreamMuxer>::Substream: Send + 'static,
    //     TTransport: Transport<Output = (TConnInfo, TMuxer)> + Clone + Send + Sync + 'static,
    //     TTransport::Error: Send + Sync + 'static,
    //     TTransport::Listener: Send + 'static,
    //     TTransport::ListenerUpgrade: Send + 'static,
    //     TTransport::Dial: Send + 'static,
    {
        Swarm {
            peers: PeerStore::default(),
            transport,
            local_peer_id,
            handler,
            listeners: SmallVec::with_capacity(16),
            next_id: ListenerId(1),

            supported_protocols: Default::default(),
            listened_addrs: Default::default(),
            external_addrs: Default::default(),
            banned_peers: Default::default(),
            connections: Default::default(),
        }
    }

    /// Returns the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        &self.transport
    }

    /// Returns information about the [`Network`] underlying the `Swarm`.
    pub fn network_info(&self) -> NetworkInfo {
        // TODO: add stats later on
        let num_connections_established = 0; //self.pool.num_established();
        let num_connections_pending = 0; //self.pool.num_pending();
        let num_connections = num_connections_established + num_connections_pending;
        let num_peers = 0; //self.pool.num_connected();
        NetworkInfo {
            num_peers,
            num_connections,
            num_connections_established,
            num_connections_pending,
        }
    }

    /// Starts listening on the given address.
    ///
    /// Returns an error if the address is not supported.
    /// TODO: addr: Multiaddr might be a Vec<Multiaddr>
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError> {
        let listener = self.transport.clone().listen_on(addr)?;

        self.listened_addrs.push(listener.multi_addr());
        //self.listeners.push(Listener::new(listener, self.next_id));

        let id = self.next_id;
        self.next_id = ListenerId(self.next_id.0 + 1);
        Ok(id)
    }

    /// Remove some listener.
    ///
    /// Returns `Ok(())` if there was a listener with this ID.
    pub fn remove_listener(&mut self, id: ListenerId) -> Result<(), ()> {
        if let Some(i) = self.listeners.iter().position(|l| l.id == id) {
            self.listeners.remove(i);
            Ok(())
        } else {
            Err(())
        }
    }

    /// Tries to dial the given address.
    ///
    /// Returns an error if the address is not supported.
    pub async fn dial_addr(&mut self, addr: &Multiaddr) -> Result<(), TransportError> {
        //let handler = self.behaviour.new_handler();
        //self.network.dial(&addr, handler.into_node_handler_builder()).map(|_id| ())

        // TODO: add dial limiter...
        let conn = self.transport().clone().dial(addr.clone()).await?;

        self.add_connection(conn, Direction::Outbound);
        Ok(())
    }

    /// Tries to initiate a dialing attempt to the given peer.
    ///
    pub async fn dial_peer(&mut self, peer_id: PeerId) -> Result<(), TransportError> {
        // Find the multiaddr of the peer
        if let Some(addrs) = self.peers.addrs.get_addr(&peer_id) {
            // TODO: handle multiple addresses

            // TODO: add dial limiter...

            let addr = addrs.first().expect("must have one");
            let conn = self.transport().clone().dial(addr.clone()).await?;

            self.add_connection(conn, Direction::Outbound);
            Ok(())
        } else {
            Err(TransportError::Internal)
        }
    }

    /*
        /// Tries to initiate a dialing attempt to the given peer.
        ///
        /// If a new dialing attempt has been initiated, `Ok(true)` is returned.
        ///
        /// If no new dialing attempt has been initiated, meaning there is an ongoing
        /// dialing attempt or `addresses_of_peer` reports no addresses, `Ok(false)`
        /// is returned.
        pub fn dial(&mut self, peer_id: &PeerId) -> Result<(), DialError> {
            let self_listening = &self.listened_addrs;
            let mut addrs = self.behaviour.addresses_of_peer(peer_id)
                .into_iter()
                .filter(|a| !self_listening.contains(a));

            let result =
                if let Some(first) = addrs.next() {
                    let handler = self.behaviour.new_handler().into_node_handler_builder();
                    self.network.peer(peer_id.clone())
                        .dial(first, addrs, handler)
                        .map(|_| ())
                        .map_err(DialError::ConnectionLimit)
                } else {
                    Err(DialError::NoAddresses)
                };

            if let Err(error) = &result {
                log::debug!(
                    "New dialing attempt to peer {:?} failed: {:?}.",
                    peer_id, error);
                self.behaviour.inject_dial_failure(&peer_id);
            }

            result
        }
        /// Returns an iterator that produces the list of addresses we're listening on.
        pub fn listeners(&self) -> impl Iterator<Item = &Multiaddr> {
            self.listeners.iter().flat_map(|l| l.addresses.iter())
        }
    */

    /// Returns an iterator that produces the list of addresses that other nodes can use to reach
    /// us.
    pub fn external_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.external_addrs.iter()
    }

    /// Returns the peer ID of the swarm passed as parameter.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Adds an external address.
    ///
    /// An external address is an address we are listening on but that accounts for things such as
    /// NAT traversal.
    pub fn add_external_address(&mut self, addr: Multiaddr) {
        self.external_addrs.add(addr)
    }
    /*
        /// Obtains a view of a [`Peer`] with the given ID in the network.
        pub fn peer(&mut self, peer_id: TPeerId)
                    -> Peer<'_, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
        {
            Peer::new(self, peer_id)
        }

        /// Returns the connection info for an arbitrary connection with the peer, or `None`
        /// if there is no connection to that peer.
        // TODO: should take &self instead of &mut self, but the API in network requires &mut
        pub fn connection_info(&mut self, peer_id: &PeerId) -> Option<TConnInfo> {
            if let Some(mut n) = self.network.peer(peer_id.clone()).into_connected() {
                Some(n.some_connection().info().clone())
            } else {
                None
            }
        }
    */
    /// Bans a peer by its peer ID.
    ///
    /// Any incoming connection and any dialing attempt will immediately be rejected.
    /// This function has no effect is the peer is already banned.
    pub fn ban_peer_id(&mut self, peer_id: PeerId) {
        self.banned_peers.insert(peer_id);

        // TODO: to disconnect
        // if let Some(c) = self.network.peer(peer_id).into_connected() {
        //     c.disconnect();
        // }
    }

    /// Unbans a peer.
    pub fn unban_peer_id(&mut self, peer_id: PeerId) {
        self.banned_peers.remove(peer_id.as_ref());
    }

    pub fn add_connection(&mut self, stream_muxer: TTrans::Output, dir: Direction) {
        let remote_peer_id = stream_muxer.remote_peer();

        // build a Connection
        let conn = Connection::new(stream_muxer, dir);

        self.connections.insert(remote_peer_id, conn);

        // TODO: we have a connection to the specified peer_id, now cancel all pending attempts

        // TODO: generate a connected event

        // TODO: start the connection in a background task

        // TODO: return the connection
    }
}

/// Connections to notify of a pending event.
///
/// The connection IDs to notify of an event are captured at the time
/// the behaviour emits the event, in order not to forward the event
/// to new connections which the behaviour may not have been aware of
/// at the time it issued the request for sending it.
#[allow(dead_code)]
enum PendingNotifyHandler {
    One(ConnectionId),
    Any(SmallVec<[ConnectionId; 10]>),
    All(SmallVec<[ConnectionId; 10]>),
}

/// The possible failures of [`Swarm::dial`].
#[derive(Debug)]
pub enum DialError {
    /// The configured limit for simultaneous outgoing connections
    /// has been reached.
    ConnectionLimit(ConnectionLimit),
    /// [`NetworkBehaviour::addresses_of_peer`] returned no addresses
    /// for the peer to dial.
    NoAddresses,
}

impl fmt::Display for DialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialError::ConnectionLimit(err) => write!(f, "Dial error: {}", err),
            DialError::NoAddresses => write!(f, "Dial error: no addresses for peer."),
        }
    }
}

impl error::Error for DialError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DialError::ConnectionLimit(err) => Some(err),
            DialError::NoAddresses => None,
        }
    }
}

#[cfg(test)]
mod tests {}
