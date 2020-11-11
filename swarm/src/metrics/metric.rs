use crate::metrics::metricmap::MetricMap;
use crate::ProtocolId;
use libp2prs_core::PeerId;
use std::fmt;
use std::ops::Add;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    protocol_in: MetricMap<ProtocolId, usize>,
    /// A hashmap that key is protocol name and value is a counter of bytes sent.
    protocol_out: MetricMap<ProtocolId, usize>,

    /// A hashmap that key is peer_id and value is a counter of bytes received.
    peer_in: MetricMap<PeerId, usize>,
    /// A hashmap that key is peer_id and value is a counter of bytes sent.
    peer_out: MetricMap<PeerId, usize>,
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
    pub(crate) fn log_sent_stream(&self, protocol: ProtocolId, count: usize, peer_id: &PeerId) {
        self.protocol_out.store_or_modify(&protocol, count, |_, value| value.add(count));
        self.peer_out.store_or_modify(peer_id, count, |_, value| value.add(count));
    }

    #[inline]
    pub(crate) fn log_recv_stream(&self, protocol: ProtocolId, count: usize, peer_id: &PeerId) {
        self.protocol_in.store_or_modify(&protocol, count, |_, value| value.add(count));
        self.peer_in.store_or_modify(peer_id, count, |_, value| value.add(count));
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
    pub fn get_protocol_in_and_out(&self, protocol_id: &ProtocolId) -> (Option<usize>, Option<usize>) {
        let protocol_in = self.protocol_in.load(protocol_id);
        let protocol_out = self.protocol_out.load(protocol_id);
        (protocol_in, protocol_out)
    }

    /// Get in&out bytes by peer_id
    pub fn get_peer_in_and_out(&self, peer_id: &PeerId) -> (Option<usize>, Option<usize>) {
        let peer_in = self.peer_in.load(peer_id);
        let peer_out = self.peer_out.load(peer_id);
        (peer_in, peer_out)
    }
}

#[cfg(test)]
mod tests {
    use crate::metrics::metric::Metric;
    use crate::ProtocolId;
    use async_std::task;
    use libp2prs_core::PeerId;
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
        let protocol = ProtocolId::default();
        task::block_on(async {
            let mut t = Vec::new();
            for i in 0..16 {
                let m = metric.clone();
                let pid = peer_id.clone();
                t.push(task::spawn(async move {
                    m.log_sent_stream(protocol, i, &pid);
                    m.log_recv_stream(protocol, i, &pid);
                }));
            }

            for item in t {
                item.await;
            }
        });

        assert_eq!(metric.get_peer_in_and_out(&peer_id), (Some(120), Some(120)));
        assert_eq!(metric.get_protocol_in_and_out(&protocol), (Some(120), Some(120)));
    }
}
