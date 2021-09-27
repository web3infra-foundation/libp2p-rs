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

// use crate::metrics::metricmap::MetricMap;
use crate::metrics::snapshot::SnapShot;
use crate::ProtocolId;
use libp2prs_core::metricmap::MetricMap;
use libp2prs_core::PeerId;
use std::collections::hash_map::IntoIter;
use std::fmt;
use std::ops::{Add, Mul, Sub};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use std::collections::HashMap;

pub struct Metric {
    /// The accumulative counter of packets sent.
    pkt_sent: AtomicUsize,
    /// The accumulative counter of packets received.
    pkt_recv: AtomicUsize,
    /// The accumulative counter of bytes sent.
    byte_sent: AtomicUsize,
    /// The accumulative counter of bytes received.
    byte_recv: AtomicUsize,

    /// A hashmap that key is protocol name and value is a counter of bytes received.
    protocol_in: MetricMap<String, Arc<AtomicUsize>>,
    /// A hashmap that key is protocol name and value is a counter of bytes sent.
    protocol_out: MetricMap<String, Arc<AtomicUsize>>,

    /// A hashmap that key is peer_id and value is a counter of bytes received.
    peer_in: MetricMap<PeerId, Arc<AtomicUsize>>,
    /// A hashmap that key is peer_id and value is a counter of bytes sent.
    peer_out: MetricMap<PeerId, Arc<AtomicUsize>>,

    recv_snapshot: Arc<RwLock<SnapShot>>,
    send_snapshot: Arc<RwLock<SnapShot>>,
}

impl fmt::Debug for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metric")
            .field("pkt_sent", &self.pkt_sent)
            .field("pkt_recv", &self.pkt_recv)
            .field("byte_sent", &self.byte_sent)
            .field("byte_recv", &self.byte_recv)
            .field("protocol_in", &self.protocol_in)
            .field("protocol_out", &self.protocol_out)
            .field("peer_in", &self.peer_in)
            .field("peer_out", &self.peer_out)
            .finish()
    }
}

impl Default for Metric {
    fn default() -> Self {
        Self::new()
    }
}

impl Metric {
    /// Create a new metric
    pub fn new() -> Metric {
        Metric {
            pkt_sent: AtomicUsize::new(0),
            pkt_recv: AtomicUsize::new(0),
            byte_sent: AtomicUsize::new(0),
            byte_recv: AtomicUsize::new(0),
            protocol_in: MetricMap::new(),
            protocol_out: MetricMap::new(),
            peer_in: MetricMap::new(),
            peer_out: MetricMap::new(),
            recv_snapshot: Arc::new(Default::default()),
            send_snapshot: Arc::new(Default::default()),
        }
    }

    #[inline]
    pub(crate) fn log_recv_msg(&self, n: usize) {
        self.pkt_recv.fetch_add(1, Ordering::SeqCst);
        self.byte_recv.fetch_add(n, Ordering::SeqCst);
    }

    #[inline]
    pub(crate) fn log_sent_msg(&self, n: usize) {
        self.pkt_sent.fetch_add(1, Ordering::SeqCst);
        self.byte_sent.fetch_add(n, Ordering::SeqCst);
    }

    #[inline]
    pub(crate) fn log_sent_stream(&self, protocol: &ProtocolId, count: usize, peer_id: &PeerId) {
        self.protocol_out
            .store_or_modify(&protocol.to_string(), Arc::new(AtomicUsize::new(count)), |_, value| { let _ = value.fetch_add(count, SeqCst); });
        self.peer_out.store_or_modify(peer_id, Arc::new(AtomicUsize::new(count)), |_, value| { let _ = value.fetch_add(count, SeqCst); });
    }

    #[inline]
    pub(crate) fn log_recv_stream(&self, protocol: &ProtocolId, count: usize, peer_id: &PeerId) {
        self.protocol_in
            .store_or_modify(&protocol.to_string(), Arc::new(AtomicUsize::new(count)), |_, value| { let _ = value.fetch_add(count, SeqCst); });
        self.peer_in.store_or_modify(peer_id, Arc::new(AtomicUsize::new(count)), |_, value| { let _ = value.fetch_add(count, SeqCst); });
    }

    /// Get count & bytes about received package
    pub fn get_recv_count_and_size(&self) -> (usize, usize) {
        (self.pkt_recv.load(SeqCst), self.byte_recv.load(SeqCst))
    }

    /// Get count & bytes about sent package
    pub fn get_sent_count_and_size(&self) -> (usize, usize) {
        (self.pkt_sent.load(SeqCst), self.byte_sent.load(SeqCst))
    }

    /// Get in&out bytes by protocol_id
    pub fn get_protocol_in_and_out(&self, protocol_id: &str) -> (Option<usize>, Option<usize>) {
        let protocol_in = self.protocol_in.load(&protocol_id.to_string()).map(|i| i.load(SeqCst));
        let protocol_out = self.protocol_out.load(&protocol_id.to_string()).map(|i| i.load(SeqCst));
        (protocol_in, protocol_out)
    }

    /// Get in&out bytes by peer_id
    pub fn get_peer_in_and_out(&self, peer_id: &PeerId) -> (Option<usize>, Option<usize>) {
        let peer_in = self.peer_in.load(peer_id).map(|count| count.load(SeqCst));
        let peer_out = self.peer_out.load(peer_id).map(|count| count.load(SeqCst));
        (peer_in, peer_out)
    }

    /// Get an iterator that key is peer_id and value is input bytes.
    pub fn get_peers_in_list(&self) -> IntoIter<PeerId, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, count)) in self.peer_in.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, count.load(SeqCst));
        };
        map.into_iter()
    }

    /// Get an iterator that key is peer_id and value is output bytes.
    pub fn get_peers_out_list(&self) -> IntoIter<PeerId, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, count)) in self.peer_out.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, count.load(SeqCst));
        };
        map.into_iter()
    }

    /// Get an iterator that key is protocol and value is input bytes.
    pub fn get_protocols_in_list(&self) -> IntoIter<String, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, count)) in self.protocol_in.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, count.load(SeqCst));
        };
        map.into_iter()
    }

    /// Get an iterator that key is protocol and value is output bytes.
    pub fn get_protocols_out_list(&self) -> IntoIter<String, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, count)) in self.protocol_out.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, count.load(SeqCst));
        };
        map.into_iter()
    }

    /// Get rates about received bytes per seconds
    pub fn get_rate_in(&self) -> f64 {
        self.recv_snapshot.read().unwrap().rate()
    }

    /// Get rates about sent bytes per seconds
    pub fn get_rate_out(&self) -> f64 {
        self.send_snapshot.read().unwrap().rate()
    }

    /// Update received and send rates
    pub fn update(&self) {
        self.update_in();
        self.update_out();
    }

    /// Update received rates
    fn update_in(&self) {
        let alpha = 1.0f64 - (-1.0f64).exp();
        let mut snapshot = self.recv_snapshot.write().unwrap();
        let now = SystemTime::now();
        let tdiff = now.duration_since(snapshot.last_update_time());
        if tdiff.is_err() {
            return;
        }
        snapshot.set_last_update_time(now);

        let time_multiplier = 1.0 / tdiff.unwrap().as_secs_f64();
        let total = self.byte_recv.load(SeqCst);
        log::trace!("Input Bytes: {}", total);
        let diff = total as i64 - snapshot.total();
        let instant = time_multiplier.mul(diff as f64);
        log::trace!("Delta Input Bytes per seconds: {}", instant);

        if snapshot.rate() == 0.0 {
            snapshot.set_rate(instant)
        } else {
            let old_rate = snapshot.rate();
            let new_rate = old_rate.add(instant.sub(old_rate).mul(alpha));
            snapshot.set_rate(new_rate)
        }
        snapshot.set_total(total as i64);
    }

    /// Update sent rates
    fn update_out(&self) {
        let alpha = 1.0f64 - (-1.0f64).exp();
        let mut snapshot = self.send_snapshot.write().unwrap();
        let now = SystemTime::now();
        let tdiff = now.duration_since(snapshot.last_update_time());
        if tdiff.is_err() {
            return;
        }
        snapshot.set_last_update_time(now);

        let time_multiplier = 1.0 / tdiff.unwrap().as_secs_f64();
        let total = self.byte_sent.load(SeqCst);
        log::trace!("Output Bytes: {}", total);
        let diff = total as i64 - snapshot.total();
        let instant = time_multiplier.mul(diff as f64);
        log::trace!("Delta Output Bytes per seconds: {}", instant);

        if snapshot.rate() == 0.0 {
            snapshot.set_rate(instant)
        } else {
            let old_rate = snapshot.rate();
            log::trace!("Old rates: {}", old_rate);
            let new_rate = old_rate.add(instant.sub(old_rate).mul(alpha));
            log::trace!("New rates: {}", new_rate);
            snapshot.set_rate(new_rate)
        }
        snapshot.set_total(total as i64);
    }
}

#[cfg(test)]
mod tests {
    use crate::metrics::metric::Metric;
    use crate::ProtocolId;
    use libp2prs_core::PeerId;
    use libp2prs_runtime::task;
    use std::sync::Arc;

    fn generate_metrics() -> Metric {
        Metric::new()
    }

    #[test]
    fn test_sent_package_and_byte() {
        let metric = Arc::new(generate_metrics());

        task::block_on(async {
            let mut t = Vec::new();
            for index in 0..16 {
                let m = metric.clone();
                t.push(task::spawn(async move {
                    m.log_sent_msg(index);
                }));
            }
            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_sent_count_and_size(), (16, 120));
    }

    #[test]
    fn test_recv_package_and_byte() {
        let metric = Arc::new(generate_metrics());

        task::block_on(async {
            let mut t = Vec::new();
            for index in 0..16 {
                let m = metric.clone();
                t.push(task::spawn(async move {
                    m.log_recv_msg(index);
                }));
            }
            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_recv_count_and_size(), (16, 120));
    }

    #[test]
    fn test_protocol_and_peer() {
        // env_logger::builder().filter_level(LevelFilter::Info).init();
        let metric = Arc::new(generate_metrics());

        let peer_id = PeerId::random();
        // let proto_id: = b"/test/1.0.0";
        let protocol = ProtocolId::new(b"/test/1.0.0", 110);
        task::block_on(async {
            let mut t = Vec::new();
            for i in 0..16 {
                let m = metric.clone();
                let pid = peer_id;
                let protocol = protocol.clone();
                t.push(task::spawn(async move {
                    m.log_sent_stream(&protocol, i, &pid);
                    m.log_recv_stream(&protocol, i, &pid);
                }));
            }

            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_peer_in_and_out(&peer_id), (Some(120), Some(120)));
        assert_eq!(metric.get_protocol_in_and_out(&protocol.to_string()), (Some(120), Some(120)));
    }
}
