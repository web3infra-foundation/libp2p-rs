use crate::{Multiaddr, PeerId, SwarmError};
use async_std::task::JoinHandle;
use libp2p_core::identity::Keypair;
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::TransportError;
use libp2p_core::PublicKey;
use smallvec::SmallVec;
use std::hash::Hash;
use std::{error::Error, fmt, io};
use crate::substream::StreamId;

/// The direction of a peer-to-peer communication channel.
#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    /// The socket comes from a dialer.
    Outbound,
    /// The socket comes from a listener.
    Inbound,
}

/// The endpoint roles associated with a peer-to-peer communication channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The socket comes from a dialer.
    Dialer,
    /// The socket comes from a listener.
    Listener,
}

impl std::ops::Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer,
        }
    }
}

impl Endpoint {
    /// Is this endpoint a dialer?
    pub fn is_dialer(self) -> bool {
        if let Endpoint::Dialer = self {
            true
        } else {
            false
        }
    }

    /// Is this endpoint a listener?
    pub fn is_listener(self) -> bool {
        if let Endpoint::Listener = self {
            true
        } else {
            false
        }
    }
}

/// The endpoint roles associated with a peer-to-peer connection.
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum ConnectedPoint {
    /// We dialed the node.
    Dialer {
        /// Multiaddress that was successfully dialed.
        address: Multiaddr,
    },
    /// We received the node.
    Listener {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Stack of protocols used to send back data to the remote.
        send_back_addr: Multiaddr,
    },
}

impl From<&'_ ConnectedPoint> for Endpoint {
    fn from(endpoint: &'_ ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl From<ConnectedPoint> for Endpoint {
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl ConnectedPoint {
    /// Turns the `ConnectedPoint` into the corresponding `Endpoint`.
    pub fn to_endpoint(&self) -> Endpoint {
        match self {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener,
        }
    }

    /// Returns true if we are `Dialer`.
    pub fn is_dialer(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => true,
            ConnectedPoint::Listener { .. } => false,
        }
    }

    /// Returns true if we are `Listener`.
    pub fn is_listener(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => false,
            ConnectedPoint::Listener { .. } => true,
        }
    }

    /// Modifies the address of the remote stored in this struct.
    ///
    /// For `Dialer`, this modifies `address`. For `Listener`, this modifies `send_back_addr`.
    pub fn set_remote_address(&mut self, new_address: Multiaddr) {
        match self {
            ConnectedPoint::Dialer { address } => *address = new_address,
            ConnectedPoint::Listener { send_back_addr, .. } => *send_back_addr = new_address,
        }
    }
}

/// Information about a successfully established connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected<I> {
    /// The connected endpoint, including network address information.
    pub endpoint: ConnectedPoint,
    /// Information obtained from the transport.
    pub info: I,
}

#[allow(dead_code)]
impl<I> Connected<I>
where
    I: ConnectionInfo,
{
    pub fn peer_id(&self) -> &I::PeerId {
        self.info.peer_id()
    }
}

/// Information about a connection.
pub trait ConnectionInfo {
    /// Identity of the node we are connected to.
    type PeerId: Eq + Hash;

    /// Returns the identity of the node we are connected to on this connection.
    fn peer_id(&self) -> &Self::PeerId;
}

impl ConnectionInfo for PeerId {
    type PeerId = PeerId;

    fn peer_id(&self) -> &PeerId {
        self
    }
}

/// Event generated by a [`Connection`].
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Event<T> {
    /// Event generated by the [`ConnectionHandler`].
    Handler(T),
    /// Address of the remote has changed.
    AddressChange(Multiaddr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(usize);

/// A multiplexed connection to a peer with associated `Substream`s.
#[allow(dead_code)]
pub struct Connection<TMuxer: StreamMuxer> {
    /// The unique ID for a connection
    id: ConnectionId,
    /// Node that handles the stream_muxer.
    stream_muxer: TMuxer,
    /// Handler that processes substreams.
    //pub(crate) substreams: SmallVec<[TMuxer::Substream; 8]>,
    substreams: SmallVec<[StreamId; 8]>,
    /// Direction of this connection
    dir: Direction,
    /// Ping service.
    ping: Option<()>,
    /// Identity service
    identity: Option<()>,    
    /// The task handle of this connection, returned by task::Spawn
    /// handle.await() when closing a connection
    handle: Option<JoinHandle<()>>,
}

impl<TMuxer: StreamMuxer> PartialEq for Connection<TMuxer> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<TMuxer> fmt::Debug for Connection<TMuxer>
where
    TMuxer: StreamMuxer + fmt::Debug,
    TMuxer::Substream: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("muxer", &self.stream_muxer)
            .field("dir", &self.dir)
            .field("subs", &self.substreams)
            .finish()
    }
}

//impl<TMuxer> Unpin for Connection<TMuxer> where TMuxer: StreamMuxer {}

#[allow(dead_code)]
impl<TMuxer> Connection<TMuxer>
where
    TMuxer: StreamMuxer + SecureInfo,
{
    /// Builds a new `Connection` from the given substream multiplexer
    /// and connection handler.
    pub fn new(id: usize, stream_muxer: TMuxer, dir: Direction) -> Self {
        Connection {
            id: ConnectionId(id),
            stream_muxer,
            dir,
            ping: None,
            substreams: Default::default(),
            handle: None,
            identity: None
        }
    }

    /// Returns the unique Id of the connection.
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// Returns a reference of the stream_muxer.
    pub fn stream_muxer(&self) -> &TMuxer {
        &self.stream_muxer
    }

    /// Sets the task handle of the connection.
    pub fn set_handle(&mut self, handle: JoinHandle<()>) {
        self.handle = Some(handle);
    }

    /// Closes the inner stream_muxer and wait for tasks.
    pub async fn close(&mut self) -> Result<(), SwarmError> {
        self.stream_muxer.close().await?;
        // wait for accept-task and bg-task to exit
        if let Some(h) = self.handle.take() {
            h.await;
        }
        Ok(())
    }

    /// local_addr is the multiaddr on our side of the connection.
    pub fn local_addr(&self) -> Multiaddr {
        self.stream_muxer.local_multiaddr()
    }

    /// remote_addr is the multiaddr on the remote side of the connection.
    pub fn remote_addr(&self) -> Multiaddr {
        self.stream_muxer.remote_multiaddr()
    }

    /// local_peer is the Peer on our side of the connection.
    pub fn local_peer(&self) -> PeerId {
        self.stream_muxer.local_peer()
    }

    /// remote_peer is the Peer on the remote side.
    pub fn remote_peer(&self) -> PeerId {
        self.stream_muxer.remote_peer()
    }

    /// local_priv_key is the public key of the peer on this side.
    pub fn local_priv_key(&self) -> Keypair {
        self.stream_muxer.local_priv_key()
    }

    /// remote_pub_key is the public key of the peer on the remote side.
    pub fn remote_pub_key(&self) -> PublicKey {
        self.stream_muxer.remote_pub_key()
    }

    /// Adds a substream id to the list.
    pub(crate) fn add_stream(&mut self, sid: StreamId) {
        log::trace!("adding sub {:?} to {:?}", sid, self);
        self.substreams.push(sid);
    }
    /// Removes a substream id from the list.
    pub(crate) fn del_stream(&mut self, sid: StreamId) {
        log::trace!("removing sub {:?} from {:?}", sid, self);
        self.substreams.retain(|id| id != &sid);
    }

    /// Returns how many substreams in the list.
    pub(crate) fn num_streams(&self) -> usize {
        self.substreams.len()
    }
}

/// Borrowed information about an incoming connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct IncomingInfo<'a> {
    /// Local connection address.
    pub local_addr: &'a Multiaddr,
    /// Stack of protocols used to send back data to the remote.
    pub send_back_addr: &'a Multiaddr,
}
#[allow(dead_code)]
impl<'a> IncomingInfo<'a> {
    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Listener {
            local_addr: self.local_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
    }
}

/// Borrowed information about an outgoing connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct OutgoingInfo<'a, TPeerId> {
    pub address: &'a Multiaddr,
    pub peer_id: Option<&'a TPeerId>,
}

#[allow(dead_code)]
impl<'a, TPeerId> OutgoingInfo<'a, TPeerId> {
    /// Builds a `ConnectedPoint` corresponding to the outgoing connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Dialer {
            address: self.address.clone(),
        }
    }
}

/// Information about a connection limit.
#[derive(Debug, Clone)]
pub struct ConnectionLimit {
    /// The maximum number of connections.
    pub limit: usize,
    /// The current number of connections.
    pub current: usize,
}

impl fmt::Display for ConnectionLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.current, self.limit)
    }
}

/// A `ConnectionLimit` can represent an error if it has been exceeded.
impl Error for ConnectionLimit {}

/// Errors that can occur in the context of an established `Connection`.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ConnectionError<THandlerErr> {
    /// An I/O error occurred on the connection.
    // TODO: Eventually this should also be a custom error?
    IO(io::Error),

    /// The connection handler produced an error.
    Handler(THandlerErr),
}

impl<THandlerErr> fmt::Display for ConnectionError<THandlerErr>
where
    THandlerErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::IO(err) => write!(f, "Connection error: I/O error: {}", err),
            ConnectionError::Handler(err) => write!(f, "Connection error: Handler error: {}", err),
        }
    }
}

impl<THandlerErr> std::error::Error for ConnectionError<THandlerErr>
where
    THandlerErr: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConnectionError::IO(err) => Some(err),
            ConnectionError::Handler(err) => Some(err),
        }
    }
}

/// Errors that can occur in the context of a pending `Connection`.
#[derive(Debug)]
#[allow(dead_code)]
pub enum PendingConnectionError {
    /// An error occurred while negotiating the transport protocol(s).
    Transport(TransportError),

    /// The peer identity obtained on the connection did not
    /// match the one that was expected or is otherwise invalid.
    InvalidPeerId,

    /// The connection was dropped because the connection limit
    /// for a peer has been reached.
    ConnectionLimit(ConnectionLimit),

    /// An I/O error occurred on the connection.
    // TODO: Eventually this should also be a custom error?
    IO(io::Error),
}

impl fmt::Display for PendingConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PendingConnectionError::IO(err) => write!(f, "Pending connection: I/O error: {}", err),
            PendingConnectionError::Transport(err) => {
                write!(f, "Pending connection: Transport error: {}", err)
            }
            PendingConnectionError::InvalidPeerId => {
                write!(f, "Pending connection: Invalid peer ID.")
            }
            PendingConnectionError::ConnectionLimit(l) => {
                write!(f, "Connection error: Connection limit: {}.", l)
            }
        }
    }
}

impl std::error::Error for PendingConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PendingConnectionError::IO(err) => Some(err),
            PendingConnectionError::Transport(err) => Some(err),
            PendingConnectionError::InvalidPeerId => None,
            PendingConnectionError::ConnectionLimit(..) => None,
        }
    }
}
