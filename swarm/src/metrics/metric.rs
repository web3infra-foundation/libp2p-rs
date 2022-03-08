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
use std::collections::HashMap;
use std::fmt;
use std::ops::{Add, Mul, Sub};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

pub struct Metric {
    /// The accumulative counter of packets and bytes.
    node_stat: RecordByteAndPacket,

    /// A map that record bytes and packets by peer_id.
    group_by_peer: MetricMap<PeerId, RecordByteAndPacket>,

    /// A map that record bytes by protocol.
    group_by_protocol: MetricMap<String, RecordByte>,

    /// Snapshot is used to calculate instant rate.
    recv_snapshot: Arc<RwLock<SnapShot>>,
    send_snapshot: Arc<RwLock<SnapShot>>,
}

/// Records count for both bytes and packets.
#[derive(Default, Clone)]
pub struct RecordByteAndPacket {
    r: RecordByte,
    packets_recv: Arc<AtomicUsize>,
    packets_sent: Arc<AtomicUsize>,
}

impl RecordByteAndPacket {
    /// Add sent counter.
    fn add_sent(&self, n: usize) {
        self.r.add_sent(n);
        self.packets_sent.fetch_add(1, SeqCst);
    }

    /// Add received counter.
    fn add_received(&self, n: usize) {
        self.r.add_received(n);
        self.packets_recv.fetch_add(1, SeqCst);
    }
}

/// Records count for bytes.
#[derive(Default, Clone)]
pub struct RecordByte {
    bytes_recv: Arc<AtomicUsize>,
    bytes_sent: Arc<AtomicUsize>,
}

impl RecordByte {
    fn add_sent(&self, n: usize) {
        self.bytes_sent.fetch_add(n, SeqCst);
    }

    fn add_received(&self, n: usize) {
        self.bytes_recv.fetch_add(n, SeqCst);
    }
}

#[derive(Debug)]
pub struct RecordByteAndPacketView {
    pub r: RecordByteView,
    pub packets_recv: usize,
    pub packets_sent: usize,
}

impl From<RecordByteAndPacket> for RecordByteAndPacketView {
    fn from(origin: RecordByteAndPacket) -> Self {
        RecordByteAndPacketView {
            r: origin.r.into(),
            packets_recv: origin.packets_recv.load(SeqCst),
            packets_sent: origin.packets_sent.load(SeqCst),
        }
    }
}

#[derive(Debug)]
pub struct RecordByteView {
    pub bytes_recv: usize,
    pub bytes_sent: usize,
}

impl From<RecordByte> for RecordByteView {
    fn from(r: RecordByte) -> Self {
        RecordByteView {
            bytes_recv: r.bytes_recv.load(SeqCst),
            bytes_sent: r.bytes_sent.load(SeqCst),
        }
    }
}

impl fmt::Debug for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metric")
            .field("pkt_sent", &self.node_stat.packets_sent.load(SeqCst))
            .field("pkt_recv", &self.node_stat.packets_recv.load(SeqCst))
            .field("byte_sent", &self.node_stat.r.bytes_sent.load(SeqCst))
            .field("byte_recv", &self.node_stat.r.bytes_recv.load(SeqCst))
            // .field("bytes_recv", &self.group_by_protocol)
            // .field("bytes_sent", &self.output.bytes_sent)
            // .field("peer_in", &self.input.peer_in)
            // .field("peer_out", &self.output.peer_out)
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
            node_stat: Default::default(),
            group_by_peer: Default::default(),
            group_by_protocol: Default::default(),
            recv_snapshot: Arc::new(Default::default()),
            send_snapshot: Arc::new(Default::default()),
        }
    }

    #[inline]
    pub(crate) fn log_recv_msg(&self, n: usize) {
        self.node_stat.add_received(n);
    }

    #[inline]
    pub(crate) fn log_sent_msg(&self, n: usize) {
        self.node_stat.add_sent(n);
    }

    #[inline]
    pub(crate) fn log_sent_stream(&self, protocol: &ProtocolId, count: usize, peer_id: &PeerId) {
        self.group_by_protocol.store_or_modify(&protocol.to_string(), |value| {
            value.add_sent(count);
        });
        self.group_by_peer.store_or_modify(peer_id, |value| value.add_sent(count));
    }

    #[inline]
    pub(crate) fn log_recv_stream(&self, protocol: &ProtocolId, count: usize, peer_id: &PeerId) {
        self.group_by_protocol.store_or_modify(&protocol.to_string(), |value| {
            value.add_received(count);
        });
        self.group_by_peer.store_or_modify(peer_id, |value| {
            value.add_received(count);
        });
    }

    /// Get count & bytes about received package
    pub fn get_recv_count_and_size(&self) -> (usize, usize) {
        (self.node_stat.packets_recv.load(SeqCst), self.node_stat.r.bytes_recv.load(SeqCst))
    }

    /// Get count & bytes about sent package
    pub fn get_sent_count_and_size(&self) -> (usize, usize) {
        (self.node_stat.packets_sent.load(SeqCst), self.node_stat.r.bytes_sent.load(SeqCst))
    }

    /// Get in&out bytes by protocol_id
    pub fn get_protocol_in_and_out(&self, protocol_id: &str) -> (Option<usize>, Option<usize>) {
        let protocol_in = self
            .group_by_protocol
            .load(&protocol_id.to_string())
            .map(|item| item.bytes_recv.load(SeqCst));
        let protocol_out = self
            .group_by_protocol
            .load(&protocol_id.to_string())
            .map(|item| item.bytes_sent.load(SeqCst));
        (protocol_in, protocol_out)
    }

    /// Get in&out bytes by peer_id
    pub fn get_peer_in_and_out(&self, peer_id: &PeerId) -> (Option<usize>, Option<usize>) {
        let peer_in = self.group_by_peer.load(peer_id).map(|item| item.r.bytes_recv.load(SeqCst));
        let peer_out = self.group_by_peer.load(peer_id).map(|item| item.r.bytes_sent.load(SeqCst));
        (peer_in, peer_out)
    }

    /// Get an iterator that key is peer_id and value is input bytes.
    pub fn get_peers_in_list(&self) -> IntoIter<PeerId, usize> {
        let mut map = HashMap::new();
        for (_, (peer, p_metric)) in self.group_by_peer.iterator().unwrap().enumerate() {
            let _ = map.insert(peer, p_metric.r.bytes_recv.load(SeqCst));
        }
        map.into_iter()
    }

    /// Get an iterator that key is peer_id and value is output bytes.
    pub fn get_peers_out_list(&self) -> IntoIter<PeerId, usize> {
        let mut map = HashMap::new();
        for (_, (peer, p_metric)) in self.group_by_peer.iterator().unwrap().enumerate() {
            let _ = map.insert(peer, p_metric.r.bytes_sent.load(SeqCst));
        }
        map.into_iter()
    }

    /// Get an iterator that key is protocol and value is input bytes.
    pub fn get_protocols_in_list(&self) -> IntoIter<String, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, p_metric)) in self.group_by_protocol.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, p_metric.bytes_recv.load(SeqCst));
        }
        map.into_iter()
    }

    /// Get an iterator that key is protocol and value is output bytes.
    pub fn get_protocols_out_list(&self) -> IntoIter<String, usize> {
        let mut map = HashMap::new();
        for (_, (protocol, p_metric)) in self.group_by_protocol.iterator().unwrap().enumerate() {
            let _ = map.insert(protocol, p_metric.bytes_sent.load(SeqCst));
        }
        map.into_iter()
    }

    pub fn get_traffic_by_peer(&self, peer_id: Option<PeerId>) -> Option<Vec<(PeerId, RecordByteAndPacketView)>> {
        if peer_id.is_none() {
            let v = self
                .group_by_peer
                .iterator()
                .unwrap()
                .map(|(p, v)| (p, v.into()))
                .collect::<Vec<(PeerId, RecordByteAndPacketView)>>();
            return Some(v);
        }
        self.group_by_peer
            .load(&peer_id.unwrap())
            .map(|p| vec![(peer_id.unwrap(), p.into())])
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
        let total = self.node_stat.r.bytes_recv.load(SeqCst);
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
        let total = self.node_stat.r.bytes_sent.load(SeqCst);
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
        env_logger::init();
        let metric = Arc::new(generate_metrics());

        task::block_on(async {
            let mut t = Vec::new();
            for index in 0..160 {
                let m = metric.clone();
                t.push(task::spawn(async move {
                    m.log_sent_msg(index);
                }));
            }
            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_sent_count_and_size(), (160, 12720));
    }

    #[test]
    fn test_recv_package_and_byte() {
        let metric = Arc::new(generate_metrics());

        task::block_on(async {
            let mut t = Vec::new();
            for index in 0..160 {
                let m = metric.clone();
                t.push(task::spawn(async move {
                    m.log_recv_msg(index);
                }));
            }
            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_recv_count_and_size(), (160, 12720));
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
            for index in 0..160 {
                let m = metric.clone();
                let pid = peer_id;
                let protocol = protocol.clone();
                t.push(task::spawn(async move {
                    m.log_sent_stream(&protocol, index, &pid);
                    m.log_recv_stream(&protocol, index, &pid);
                }));
            }

            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_peer_in_and_out(&peer_id), (Some(12720), Some(12720)));
        assert_eq!(metric.get_protocol_in_and_out(&protocol.to_string()), (Some(12720), Some(12720)));
    }
}
