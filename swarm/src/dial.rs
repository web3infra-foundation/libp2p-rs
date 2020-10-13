use async_std::task;
use async_std::task::JoinHandle;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use smallvec::SmallVec;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{error::Error, fmt};

use crate::Connection;
use async_trait::async_trait;
use fnv::FnvHashMap;
use libp2p_core::{muxing::StreamMuxer, transport::TransportError, Multiaddr, PeerId};

type Result<T> = std::result::Result<T, DialError>;

// CONCURRENT_FD_DIALS is the number of concurrent outbound dials over transports
// that consume file descriptors
const CONCURRENT_FD_DIALS: usize = 160;

// DEFAULT_PER_PEER_RATE_LIMIT is the number of concurrent outbound dials to make
// per peer
const DEFAULT_PER_PEER_RATE_LIMIT: usize = 8;

#[derive(Debug)]
pub enum DialError {
    // been dialed too frequently
    ErrDialBackoff,

    // ErrDialToSelf is returned if we attempt to dial our own peer
    ErrDialToSelf,

    // ErrNoTransport is returned when we don't know a transport for the
    // given multiaddr.
    ErrNoTransport,

    // ErrAllDialsFailed is returned when connecting to a peer has ultimately failed
    ErrAllDialsFailed,

    // ErrNoAddresses is returned when we fail to find any addresses for a
    // peer we're trying to dial.
    ErrNoAddresses,

    // ErrNoGoodAddresses is returned when we find addresses for a peer but
    // can't use any of them.
    ErrNoGoodAddresses,

    // ErrGaterDisallowedConnection is returned when the gater prevents us from
    // forming a connection with a peer.
    ErrGaterDisallowedConnection,
}

struct DialResult<TStreamMuxer: StreamMuxer> {
    conn: Connection<TStreamMuxer>,
    addr: Multiaddr,
}

struct DialJob {
    addr: Multiaddr,
    peer: PeerId,
    resp: oneshot::Sender<Result<()>>,
}

struct DialLimiter<TStreamMuxer: StreamMuxer> {
    inner: std::sync::Mutex<DialLimiterInner<TStreamMuxer>>,
}

pub type IDialHander<TStreamMuxer> = Box<dyn Dial<TStreamMuxer> + Send + Sync>;

#[async_trait]
pub trait Dial<TStreamMuxer: StreamMuxer> {
    async fn dial_addr(&mut self, addr: Multiaddr) -> Result<Connection<TStreamMuxer>>;
}

struct DialLimiterInner<TStreamMuxer> {
    dial_handler: IDialHander<TStreamMuxer>,
    fd_consuming: u32,
    fd_limit: u32,
    waiting_on_fd: Vec<DialJob>,
    active_per_peer: FnvHashMap<PeerId, u32>,
    per_peer_limit: u32,
    waiting_on_peer_limit: FnvHashMap<PeerId, Vec<DialJob>>,
}

//todo
