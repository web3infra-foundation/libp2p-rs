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

use fnv::FnvHashMap;
use smallvec::SmallVec;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::channel::mpsc::UnboundedReceiver;
use futures::lock::Mutex;
use futures::prelude::*;

use libp2prs_core::muxing::IStreamMuxer;
use libp2prs_core::transport::upgrade::ITransportEx;
use libp2prs_core::{
    multiaddr::{protocol, Multiaddr},
    PeerId,
};
use libp2prs_runtime::task;

use crate::connection::Direction;
use crate::{SwarmError, SwarmEvent, TransactionId, Transports};
use libp2prs_core::routing::IRouting;

type Result<T> = std::result::Result<T, SwarmError>;

/// CONCURRENT_DIALS_LIMIT  is the number of concurrent outbound dials
const CONCURRENT_DIALS_LIMIT: u32 = 1000;

/// DIAL_TIMEOUT is the maximum duration a Dial is allowed to take.This includes the time between dialing the raw network connection,protocol selection as well the handshake, if applicable.
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);

/// DIAL_TIMEOUT_LOCAL is the maximum duration a Dial to local network address is allowed to take.This includes the time between dialing the raw network connection,protocol selection as well the handshake, if applicable.
const DIAL_TIMEOUT_LOCAL: Duration = Duration::from_secs(2);

/// DIAL_ATTEMPTS is the maximum dial attempts (default: 1).
const DIAL_ATTEMPTS: u32 = 1;

/// BACKOFF_BASE is the base amount of time to backoff (default: 5s).
const BACKOFF_BASE: Duration = Duration::from_secs(5);

/// BACKOFF_COEF is the backoff coefficient (default: 1s).
const BACKOFF_COEF: Duration = Duration::from_secs(1);

/// BACKOFF_MAX is the maximum backoff time (default: 300s).
const BACKOFF_MAX: Duration = Duration::from_secs(300);

// const PUBLIC_IPS: [&str; 4] = ["47.75.221.15", "47.244.137.22", "47.244.56.200", "47.90.106.218"];

/// Statistics of dialer.
#[derive(Default)]
struct DialerStats {
    total_attempts: AtomicUsize,
    total_success: AtomicUsize,
    no_address: AtomicUsize,
    no_transport: AtomicUsize,
    dialing_backoff: AtomicUsize,
    total_jobs: AtomicUsize,
    dialing_success: AtomicUsize,
    dialing_limit: AtomicUsize,
    dialing_timeout: AtomicUsize,
    mismatch_peer_id: AtomicUsize,
    transport_error: AtomicUsize,
}

#[derive(Debug)]
pub struct DialerStatsView {
    /// Dialing in progress.
    pub in_progress: usize,
    /// The total dialing attempts.
    pub total_attempts: usize,
    /// The total successful dialing attempts.
    pub total_success: usize,
    /// Addresses not found via DHT.
    pub no_address: usize,
    /// No suitable transport found for the peer to be dialed.
    pub no_transport: usize,
    /// All addresses of the peer are in backoff.
    pub dialing_backoff: usize,
    /// Total dialing jobs.
    pub total_jobs: usize,
    /// Successful jobs.
    pub dialing_success: usize,
    /// Reaches to the concurrent dialing limit.
    pub dialing_limit: usize,
    /// Dialing not finished in a give deadline.
    pub dialing_timeout: usize,
    /// Got a tranport error when dialing.
    pub transport_error: usize,
    /// Remote peer reports a mismatched peer ID.
    /// Note: this counter is counted into dialing_success.
    pub mismatch_peer_id: usize,
}

#[derive(Clone)]
struct DialJob {
    transport: ITransportEx,
    addr: Multiaddr,
    peer: PeerId,
    stats: Arc<DialerStats>,
    // here we send the maddr back for dialer-backoff
    tx: mpsc::UnboundedSender<(Result<IStreamMuxer>, Multiaddr, Duration)>,
}

#[derive(Clone)]
struct DialLimiter {
    dial_consuming: Arc<AtomicU32>,
    dial_limit: u32,
}

impl DialLimiter {
    fn new() -> Self {
        let mut dial_limit = CONCURRENT_DIALS_LIMIT;
        for (key, value) in std::env::vars() {
            if key == "LIBP2P_SWARM_DIAL_LIMIT" {
                dial_limit = value.parse::<u32>().unwrap();
            }
        }
        DialLimiter {
            dial_consuming: Arc::new(AtomicU32::new(0)),
            dial_limit,
        }
    }

    fn dial_timeout(&self, ma: &Multiaddr) -> Duration {
        let mut timeout: Duration = DIAL_TIMEOUT;
        if ma.is_private_addr() {
            timeout = DIAL_TIMEOUT_LOCAL;
        }
        timeout
    }

    /// Tries to take the needed tokens for starting the given dial job.
    async fn do_dial_job(&self, mut dj: DialJob) {
        log::debug!("[DialLimiter] executing job through limiter, {} {}", dj.peer, dj.addr);

        if self.dial_consuming.load(Ordering::SeqCst) >= self.dial_limit {
            log::debug!(
                "[DialLimiter] Terminate while waiting on dial token; peer: {}; addr: {}; consuming: {:?}; limit: {:?};",
                dj.peer,
                dj.addr,
                self.dial_consuming,
                self.dial_limit,
            );
            dj.stats.dialing_limit.fetch_add(1, Ordering::SeqCst);
            let _ = dj
                .tx
                .send((Err(SwarmError::ConcurrentDialLimit(self.dial_limit)), dj.addr, Default::default()))
                .await;
            return;
        }

        log::trace!(
            "[DialLimiter] taking token: peer: {}; addr: {}; prev consuming: {:?}",
            dj.peer,
            dj.addr,
            self.dial_consuming
        );
        self.dial_consuming.fetch_add(1, Ordering::SeqCst);
        self.execute_dial(dj).await;
    }

    // execute_dial calls the do_dial method to dial, and reports the result through the response
    // channel when finished. Once the response is sent it also releases all tokens
    // it held during the dial.
    async fn execute_dial(&self, mut dj: DialJob) {
        let timeout = self.dial_timeout(&dj.addr);

        let start = Instant::now();
        let dial_r = task::timeout(timeout, dj.transport.dial(dj.addr.clone())).await;
        let cost = start.elapsed();
        if let Ok(r) = dial_r {
            if r.is_err() {
                dj.stats.transport_error.fetch_add(1, Ordering::SeqCst);
            } else {
                dj.stats.dialing_success.fetch_add(1, Ordering::SeqCst);
            }
            let _ = dj.tx.send((r.map_err(|e| e.into()), dj.addr, cost)).await;
        } else {
            dj.stats.dialing_timeout.fetch_add(1, Ordering::SeqCst);
            let _ = dj
                .tx
                .send((Err(SwarmError::DialTimeout(dj.addr.clone(), timeout.as_secs())), dj.addr, cost))
                .await;
        }
        self.dial_consuming.fetch_sub(1, Ordering::SeqCst);
    }
}

/// DialBackoff is a type for tracking peer dial backoffs.
///
/// * It's thread-safe.
#[derive(Clone)]
pub(crate) struct DialBackoff {
    entries: Arc<Mutex<FnvHashMap<PeerId, FnvHashMap<String, BackoffAddr>>>>,
    max_time: Option<Duration>,
}

#[derive(Clone, Debug)]
struct BackoffAddr {
    tries: u32,
    until: Instant,
}

#[allow(dead_code)]
impl DialBackoff {
    fn new() -> Self {
        Self {
            entries: Default::default(),
            max_time: None,
        }
    }

    fn with_max_time(mut self, time: Duration) -> Self {
        self.max_time = Some(time);
        self
    }

    /// Returns whether the client should backoff dialing peer at address
    async fn find_peer(&self, peer_id: &PeerId, ma: &Multiaddr) -> bool {
        log::debug!("[DialBackoff] lookup checking, addr={:?}", ma);
        let lock = self.entries.lock().await;
        if let Some(peer_map) = lock.get(peer_id) {
            if let Some(backoff) = peer_map.get(&ma.to_string()) {
                log::debug!(
                    "[DialBackoff] backoff found: Instant={:?}, ma={:?}, backoff={:?}",
                    Instant::now(),
                    ma,
                    backoff
                );
                return Instant::now() < backoff.until;
            }
        }
        false
    }

    /// Let other nodes know that we've entered backoff with peer p, so dialers should not wait unnecessarily.
    /// We still will attempt to dial with runtime::spawn, in case we get through.
    ///
    /// Backoff is not exponential, it's quadratic and computed according to the following formula:
    ///
    /// BackoffBase + BakoffCoef * PriorBackoffs^2
    ///
    /// Where PriorBackoffs is the number of previous backoffs.
    async fn add_peer(&self, peer_id: PeerId, ma: Multiaddr) {
        let mut lock = self.entries.lock().await;
        let peer_map = lock.entry(peer_id).or_insert_with(Default::default);
        if let Some(backoff) = peer_map.get_mut(&ma.to_string()) {
            let mut backoff_time = BACKOFF_BASE + BACKOFF_COEF * (backoff.tries * backoff.tries);
            if backoff_time > BACKOFF_MAX {
                backoff_time = BACKOFF_MAX
            }
            backoff.until = Instant::now() + backoff_time;
            backoff.tries += 1;
            log::debug!("[DialBackoff] adding backoff {:?}", backoff);
        } else {
            let until = Instant::now() + BACKOFF_BASE;
            let backoff = peer_map.insert(ma.to_string(), BackoffAddr { tries: 1, until });
            log::debug!("[DialBackoff] updating backoff {:?}", backoff);
        }
    }

    // backoff background runtime
    // It cleans up the backoff list periodically, or exits when Dialer is closing the channel
    fn start_cleanup_task(&self) -> mpsc::Sender<()> {
        let (tx, mut rx) = mpsc::channel(0);

        let me = self.clone();
        task::spawn(async move {
            log::info!("[DialBackoff] starting backoff background runtime...");

            let interval = me.max_time.unwrap_or(BACKOFF_MAX);
            loop {
                let res = task::timeout(interval, rx.next()).await;
                match res {
                    Ok(_) => {
                        // we are closed anyway, break
                        log::info!("[DialBackoff] closed, exiting...");
                        break;
                    }
                    Err(_) => {
                        log::trace!("[DialBackoff] cleaning up backoff...");
                        me.clone().do_cleanup().await;
                    }
                }
            }
        });
        tx
    }

    async fn do_cleanup(self) {
        let clean_peer_ids = {
            let mut clean_peer_ids = vec![];
            let now = Instant::now();
            let lock = self.entries.lock().await;
            for (p, e) in lock.iter() {
                let mut good = false;
                for backoff in e.values() {
                    let backoff_time = Duration::min(BACKOFF_BASE + BACKOFF_COEF * (backoff.tries * backoff.tries), BACKOFF_MAX);

                    log::debug!(
                        "[DialBackoff] now={:?} backoff.until + backoff_time={:?}",
                        now,
                        (backoff.until + backoff_time)
                    );
                    if now < backoff.until + backoff_time {
                        good = true;
                        break;
                    }
                }
                if !good {
                    clean_peer_ids.push(*p);
                }
            }
            clean_peer_ids
        };
        let mut lock = self.entries.lock().await;
        for id in clean_peer_ids {
            let _ = lock.remove(&id);
        }
    }
}

/// Represents whether dialing with addresses or using DHT to find peer
#[derive(Clone)]
#[allow(clippy::upper_case_acronyms)]
pub(crate) enum EitherDialAddr {
    Addresses(Vec<Multiaddr>),
    DHT(IRouting),
}

pub(crate) struct AsyncDialer {
    limiter: DialLimiter,
    backoff: DialBackoff,
    stats: Arc<DialerStats>,
    handle: mpsc::Sender<()>,
    attempts: u32,
}

#[derive(Clone)]
struct DialParam {
    transports: Transports,
    addrs: EitherDialAddr,
    peer_id: PeerId,
    tid: TransactionId,
    limiter: DialLimiter,
    backoff: DialBackoff,
    stats: Arc<DialerStats>,
    attempts: u32,
    filter_private: bool,
    filter_loopback: bool,
}

impl Drop for AsyncDialer {
    fn drop(&mut self) {
        log::info!("terminating backoff background runtime...");
        self.handle.close_channel();
    }
}

impl AsyncDialer {
    pub(crate) fn new() -> Self {
        let mut attempts = DIAL_ATTEMPTS;
        for (key, value) in std::env::vars() {
            if key == "LIBP2P_SWARM_DIAL_ATTEMPTS" {
                attempts = value.parse::<u32>().unwrap();
            }
        }

        let limiter = DialLimiter::new();
        let backoff = DialBackoff::new();

        // start the background runtime for backoff
        let handle = backoff.start_cleanup_task();

        Self {
            limiter,
            backoff,
            attempts,
            handle,
            stats: Arc::new(Default::default()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dial(
        &self,
        peer_id: PeerId,
        transports: Transports,
        addrs: EitherDialAddr,
        mut event_sender: mpsc::UnboundedSender<SwarmEvent>,
        tid: TransactionId,
        filter_private: bool,
        filter_loopback: bool,
    ) {
        let dial_param = DialParam {
            transports,
            addrs,
            peer_id,
            tid,
            limiter: self.limiter.clone(),
            backoff: self.backoff.clone(),
            stats: self.stats.clone(),
            attempts: self.attempts,
            filter_private,
            filter_loopback,
        };

        self.stats.total_attempts.fetch_add(1, Ordering::SeqCst);

        task::spawn(async move {
            let tid = dial_param.tid;
            let peer_id = dial_param.peer_id;
            let stats = dial_param.stats.clone();

            let r = AsyncDialer::start_dialing(dial_param).await;
            match r {
                Ok(stream_muxer) => {
                    stats.total_success.fetch_add(1, Ordering::SeqCst);
                    let _ = event_sender
                        .send(SwarmEvent::ConnectionEstablished {
                            stream_muxer,
                            direction: Direction::Outbound,
                            tid: Some(tid),
                        })
                        .await;
                }
                Err(err) => {
                    let _ = event_sender
                        .send(SwarmEvent::OutgoingConnectionError { tid, peer_id, error: err })
                        .await;
                }
            }
        });
    }

    pub(crate) fn stats(&self) -> DialerStatsView {
        DialerStatsView {
            in_progress: self.limiter.dial_consuming.load(Ordering::SeqCst) as usize,
            total_attempts: self.stats.total_attempts.load(Ordering::SeqCst),
            total_success: self.stats.total_success.load(Ordering::SeqCst),
            no_address: self.stats.no_address.load(Ordering::SeqCst),
            no_transport: self.stats.no_transport.load(Ordering::SeqCst),
            dialing_backoff: self.stats.dialing_backoff.load(Ordering::SeqCst),
            total_jobs: self.stats.total_jobs.load(Ordering::SeqCst),
            dialing_success: self.stats.dialing_success.load(Ordering::SeqCst),
            dialing_limit: self.stats.dialing_limit.load(Ordering::SeqCst),
            dialing_timeout: self.stats.dialing_timeout.load(Ordering::SeqCst),
            mismatch_peer_id: self.stats.mismatch_peer_id.load(Ordering::SeqCst),
            transport_error: self.stats.transport_error.load(Ordering::SeqCst),
        }
    }

    async fn start_dialing(dial_param: DialParam) -> Result<IStreamMuxer> {
        let mut dial_count: u32 = 0;
        loop {
            dial_count += 1;

            let active_param = dial_param.clone();
            let r = AsyncDialer::dial_addrs(active_param).await;
            if let Err(e) = r {
                log::debug!("[Dialer] dialer failed at attempt={} error={:?}", dial_count, e);
                if dial_count < dial_param.attempts {
                    log::error!(
                        "[Dialer] All addresses of {:?} cannot be dialed to. Now try dialing again, attempts={}",
                        dial_param.peer_id,
                        dial_count
                    );
                    //TODO:
                    task::sleep(BACKOFF_BASE).await;
                } else if dial_param.attempts > 1 {
                    break Err(SwarmError::MaxDialAttempts(dial_param.attempts));
                } else {
                    break Err(e);
                }
            } else {
                break r;
            }
        }
    }

    /// Starts a dialing runtime
    async fn dial_addrs(mut param: DialParam) -> Result<IStreamMuxer> {
        let peer_id = param.peer_id;
        log::debug!("[Dialer] dialing for {:?}", peer_id);

        let mut addrs_origin = match &mut param.addrs {
            EitherDialAddr::Addresses(addrs) => addrs.clone(),
            EitherDialAddr::DHT(routing) => match routing.find_peer(&peer_id).await {
                Ok(addrs) => addrs,
                Err(err) => {
                    param.stats.no_address.fetch_add(1, Ordering::SeqCst);
                    return Err(err.into());
                }
            },
        };

        if param.filter_private {
            addrs_origin = addrs_origin.into_iter().filter(|addr| !addr.is_private_addr()).collect();
        }

        if param.filter_loopback {
            addrs_origin = addrs_origin.into_iter().filter(|addr| !addr.is_loopback_addr()).collect();
        }

        // TODO: filter Known Undialables address ,If there is no address  can dial return SwarmError::NoGoodAddresses

        // Check backoff, make a new empty vec at first
        let mut addrs = SmallVec::new();
        for addr in addrs_origin.iter() {
            // skip addresses in back-off
            if !param.backoff.find_peer(&peer_id, addr).await {
                addrs.push(addr.clone());
            }
        }

        if addrs.is_empty() {
            log::debug!(
                "unfortunately all {} addresses for {:?} are in backoff list, failed",
                addrs_origin.len(),
                peer_id
            );
            param.stats.dialing_backoff.fetch_add(1, Ordering::SeqCst);
            return Err(SwarmError::DialBackoff);
        }

        // ranking all addresses
        let addrs_rank = AsyncDialer::rank_addrs(addrs);

        // dialing all addresses
        let (tx, rx) = mpsc::unbounded::<(Result<IStreamMuxer>, Multiaddr, Duration)>();
        let mut num_jobs = 0;

        for addr in addrs_rank {
            // first of all, check the transport
            let r = param.transports.lookup_by_addr(addr.clone());
            if r.is_err() {
                log::debug!("[Dialer] no transport found for {:?} {:?}", peer_id, addr);
                continue;
            }

            num_jobs += 1;

            let dj = DialJob {
                addr,
                peer: peer_id,
                stats: param.stats.clone(),
                tx: tx.clone(),
                transport: r.unwrap(),
            };
            // spawn a runtime to dial
            let limiter = param.limiter.clone();
            task::spawn(async move {
                limiter.do_dial_job(dj).await;
            });
        }

        if num_jobs > 0 {
            log::debug!("total {} dialing jobs for {:?} started, collecting...", num_jobs, peer_id);
            param.stats.total_jobs.fetch_add(num_jobs, Ordering::SeqCst);
            AsyncDialer::collect_dialing_result(rx, num_jobs, param).await
        } else {
            param.stats.no_transport.fetch_add(1, Ordering::SeqCst);
            Err(SwarmError::DialNoTransport(peer_id))
        }
    }

    // collect the job results
    // return the first successful dialing result, ignore the rest
    async fn collect_dialing_result(
        mut rx: UnboundedReceiver<(Result<IStreamMuxer>, Multiaddr, Duration)>,
        jobs: usize,
        param: DialParam,
    ) -> Result<IStreamMuxer> {
        for i in 0..jobs {
            let peer_id = param.peer_id;
            let r = rx.next().await;
            log::debug!("[Dialer] job for {:?} finished, seq={} ...", peer_id, i);

            match r {
                Some((Ok(stream_muxer), addr, _cost)) => {
                    let reported_pid = stream_muxer.remote_peer();

                    // verify if the PeerId matches expectation, otherwise,
                    // it is a bad outgoing connection
                    if peer_id == reported_pid {
                        log::debug!("[Dialer] job for {:?} succeeded, {:?}", peer_id, stream_muxer);
                        // return here, ignore the rest of jobs
                        return Ok(stream_muxer);
                    } else {
                        log::debug!(
                            "[Dialer] job failed due to peer id mismatch conn={:?} wanted={:?} got={:?}",
                            stream_muxer,
                            peer_id,
                            reported_pid
                        );
                        param.stats.mismatch_peer_id.fetch_add(1, Ordering::SeqCst);
                        param.backoff.add_peer(peer_id, addr).await;
                    }
                }
                Some((Err(err), addr, cost)) => {
                    log::debug!(
                        "[Dialer] job for {:?}, cost {:?}, failed: addr={:?},error={:?}",
                        peer_id,
                        cost,
                        addr,
                        err
                    );
                    if let SwarmError::Transport(_) = err {
                        // add to backoff list if transport error reported
                        param.backoff.add_peer(peer_id, addr).await;
                    }
                }
                None => {
                    log::warn!("[Dialer] should not happen");
                }
            }
        }

        Err(SwarmError::AllDialsFailed)
    }

    /// ranks addresses in descending order of preference for dialing   Private UDP > Public UDP > Private TCP > Public TCP > UDP Relay server > TCP Relay server
    fn rank_addrs(addrs: SmallVec<[Multiaddr; 4]>) -> Vec<Multiaddr> {
        let mut local_udp_addrs = Vec::<Multiaddr>::new(); // private udp
        let mut relay_udp_addrs = Vec::<Multiaddr>::new(); // relay udp
        let mut others_udp = Vec::<Multiaddr>::new(); // public udp
        let mut local_fd_addrs = Vec::<Multiaddr>::new(); // private fd consuming
        let mut relay_fd_addrs = Vec::<Multiaddr>::new(); //  relay fd consuming
        let mut others_fd = Vec::<Multiaddr>::new(); // public fd consuming
        let mut relays = Vec::<Multiaddr>::new();
        let mut fds = Vec::<Multiaddr>::new();
        let mut rank = Vec::<Multiaddr>::new();
        for addr in addrs.into_iter() {
            if addr.value_for_protocol(protocol::P2P_CIRCUIT).is_some() {
                if addr.should_consume_fd() {
                    relay_fd_addrs.push(addr);
                    continue;
                }
                relay_udp_addrs.push(addr);
            } else if addr.is_private_addr() {
                if addr.should_consume_fd() {
                    local_fd_addrs.push(addr);
                    continue;
                }
                local_udp_addrs.push(addr);
            } else {
                if addr.should_consume_fd() {
                    others_fd.push(addr);
                    continue;
                }
                others_udp.push(addr);
            }
        }
        relays.append(&mut relay_udp_addrs);
        relays.append(&mut relay_fd_addrs);
        fds.append(&mut local_fd_addrs);
        fds.append(&mut others_fd);
        rank.append(&mut local_udp_addrs);
        rank.append(&mut others_udp);
        rank.append(&mut fds);
        rank.append(&mut relays);
        rank
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2prs_core::{Multiaddr, PeerId};
    use std::str::FromStr;
    use std::time::Duration;

    #[test]
    fn test_dial_find_peer() {
        let ab = DialBackoff::new();
        let peer_id = PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
        let dial_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/8086").unwrap();
        let r = task::block_on(async {
            ab.add_peer(peer_id, dial_addr.clone()).await;
            ab.find_peer(&peer_id, &dial_addr).await
        });
        assert!(r);
    }

    #[test]
    fn test_dial_isnot_backoff() {
        let ab = DialBackoff::new();
        let peer_id = PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
        let dial_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/8086").unwrap();
        let r = task::block_on(async {
            ab.add_peer(peer_id, dial_addr.clone()).await;
            let backoff_time = {
                let lock = ab.entries.lock().await;
                let backoff_addr = lock.get(&peer_id).unwrap().get(&dial_addr.to_string()).unwrap();
                BACKOFF_BASE + BACKOFF_COEF * (backoff_addr.tries * backoff_addr.tries)
            };
            task::sleep(backoff_time).await;
            ab.find_peer(&peer_id, &dial_addr).await
        });
        assert!(!r);
    }

    #[test]
    fn test_dial_backoff_cleanup() {
        let ab = DialBackoff::new().with_max_time(Duration::from_secs(12));
        let mut h = ab.start_cleanup_task();

        let peer_id = PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
        let dial_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/8086").unwrap();
        let ab_clone = ab.clone();
        let r1 = task::block_on(async {
            ab_clone.add_peer(peer_id, dial_addr.clone()).await;
            ab_clone.find_peer(&peer_id, &dial_addr).await
        });
        let r2 = task::block_on(async {
            task::sleep(Duration::from_secs(13)).await;
            ab.entries.lock().await.get(&peer_id).is_none()
        });

        h.close_channel();

        assert!(r1);
        assert!(r2);
    }

    #[test]
    fn test_dial_backoff_cleanup_task_exit() {
        let ab = DialBackoff::new().with_max_time(Duration::from_secs(12));
        let mut h = ab.start_cleanup_task();

        let peer_id = PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
        let dial_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/8086").unwrap();
        let ab_clone = ab.clone();
        let r1 = task::block_on(async {
            ab_clone.add_peer(peer_id, dial_addr.clone()).await;
            ab_clone.find_peer(&peer_id, &dial_addr).await
        });

        h.close_channel();

        let r2 = task::block_on(async {
            task::sleep(Duration::from_secs(13)).await;
            ab.entries.lock().await.get(&peer_id).is_none()
        });

        assert!(r1);
        assert!(!r2);
    }
}
