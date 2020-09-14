use crate::{Multiaddr, PeerId};
use libp2p_core::identity::Keypair;
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::TransportError;
use libp2p_core::PublicKey;
use smallvec::SmallVec;
use std::hash::Hash;
use std::{error::Error, fmt, io};

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

/// A multiplexed connection to a peer with associated `Substream`s.
#[allow(dead_code)]
pub struct Connection<TMuxer>
where
    TMuxer: StreamMuxer,
{
    /// Node that handles the muxer.
    muxer: TMuxer,
    /// Handler that processes substreams.
    substreams: SmallVec<[TMuxer::Substream; 8]>,
    /// Direction of this connection
    dir: Direction,
}

impl<TMuxer> fmt::Debug for Connection<TMuxer>
where
    TMuxer: StreamMuxer + fmt::Debug,
    TMuxer::Substream: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("muxer", &self.muxer)
            .field("substreams", &self.substreams)
            .finish()
    }
}

impl<TMuxer> Unpin for Connection<TMuxer> where TMuxer: StreamMuxer {}

#[allow(dead_code)]
impl<TMuxer> Connection<TMuxer>
where
    TMuxer: StreamMuxer + SecureInfo,
{
    /// Builds a new `Connection` from the given substream multiplexer
    /// and connection handler.
    pub fn new(muxer: TMuxer, dir: Direction) -> Self {
        Connection {
            muxer,
            dir,
            substreams: Default::default(),
        }
    }

    /// local_peer is the Peer on our side of the connection
    pub fn local_peer(&self) -> PeerId {
        self.muxer.local_peer()
    }

    /// remote_peer is the Peer on the remote side
    pub fn remote_peer(&self) -> PeerId {
        self.muxer.remote_peer()
    }

    /// local_priv_key is the public key of the peer on this side
    pub fn local_priv_key(&self) -> Keypair {
        self.muxer.local_priv_key()
    }

    /// remote_pub_key is the public key of the peer on the remote side
    pub fn remote_pub_key(&self) -> PublicKey {
        self.muxer.remote_pub_key()
    }

    fn add_stream(&mut self, ss: TMuxer::Substream, _dir: Endpoint) -> Result<(), ()> {
        self.substreams.push(ss);

        // TODO: generate STREAM_OPENED event

        Ok(())
    }

    /// new_stream returns a new Stream from this connection
    ///
    pub async fn new_stream(&mut self) -> Result<TMuxer::Substream, TransportError> {
        let ss = self.muxer.open_stream().await?;
        //self.add_stream(ss.clone(), Endpoint::Dialer);

        Ok(ss)
    }

    /// new_stream returns a new Stream from this connection
    ///
    pub async fn accept_stream(&mut self) -> Result<TMuxer::Substream, TransportError> {
        let ss = self.muxer.accept_stream().await?;
        //self.add_stream(ss.clone(), Endpoint::Listener);

        Ok(ss)
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

/// Connection identifier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(usize);

#[allow(dead_code)]
impl ConnectionId {
    /// Creates a `ConnectionId` from a non-negative integer.
    ///
    /// This is primarily useful for creating connection IDs
    /// in test environments. There is in general no guarantee
    /// that all connection IDs are based on non-negative integers.
    pub fn new(id: usize) -> Self {
        ConnectionId(id)
    }
}