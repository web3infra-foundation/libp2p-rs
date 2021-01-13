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

use futures::executor::block_on;
use libp2prs_swarm::Control;
use prometheus::core::{Collector, Desc};
use prometheus::Counter;
use prometheus::{proto, CounterVec, Opts};
use std::collections::HashMap;

/// Exporter is used to expose Metrics data.
pub(crate) struct Exporter {
    control: Control,
    desc: Desc,
}

impl Collector for Exporter {
    fn desc(&self) -> Vec<&Desc> {
        vec![&self.desc]
    }

    fn collect(&self) -> Vec<proto::MetricFamily> {
        let mut family = Vec::<proto::MetricFamily>::new();

        // recv data
        let recv_count = Counter::new("recv_pkg_counts", "Total number of packages received").unwrap();
        let recv_size = Counter::new("recv_pkg_bytes", "Total bytes of packages received").unwrap();
        let (count, size) = self.control.get_recv_count_and_size();
        recv_count.inc_by(count as f64);
        recv_size.inc_by(size as f64);
        family.push(recv_count.collect()[0].clone());
        family.push(recv_size.collect()[0].clone());

        // send data
        let sent_count = Counter::new("sent_pkg_counts", "Total number of packages sent").unwrap();
        let sent_size = Counter::new("sent_pkg_bytes", "Total size of packages sent").unwrap();
        let (count, size) = self.control.get_sent_count_and_size();
        sent_count.inc_by(count as f64);
        sent_size.inc_by(size as f64);
        family.push(sent_count.collect()[0].clone());
        family.push(sent_size.collect()[0].clone());

        // connection data
        let mut state = self.control.clone();
        block_on(async {
            let info = state.retrieve_networkinfo().await.unwrap();
            let connect_peer = Counter::new("peer_count", "Current number of connection peers").unwrap();
            connect_peer.inc_by(info.num_peers as f64);
            family.push(connect_peer.collect()[0].clone());
            let connection_count = Counter::new("connection_count", "Current number of connections").unwrap();
            connection_count.inc_by(info.num_connections as f64);
            family.push(connection_count.collect()[0].clone());
            let connection_pending = Counter::new("connection_pending", "Current num of pending connections").unwrap();
            connection_pending.inc_by(info.num_connections_pending as f64);
            family.push(connection_pending.collect()[0].clone());
            let connection_established = Counter::new("connection_established", "Current num of established connections").unwrap();
            connection_established.inc_by(info.num_connections_established as f64);
            family.push(connection_established.collect()[0].clone());
            let active_stream = Counter::new("active_stream", "Current num of active streams").unwrap();
            active_stream.inc_by(info.num_active_streams as f64);
            family.push(active_stream.collect()[0].clone());
        });

        // peer_in
        let opt = Opts::new("peer_id_input_bytes", "Each peer input bytes");
        let peer = CounterVec::new(opt, &["peer_id"]).unwrap();
        for (k, v) in self.control.peer_in_iter() {
            let mut map = HashMap::new();
            let value = k.to_string();
            map.insert("peer_id", value.as_str());
            peer.with(&map).inc_by(v as f64);
        }
        family.push(peer.collect()[0].clone());

        // peer_out
        let opt = Opts::new("peer_id_output_bytes", "Each peer output bytes");
        let peer = CounterVec::new(opt, &["peer_id"]).unwrap();
        for (k, v) in self.control.peer_out_iter() {
            let mut map = HashMap::new();
            let value = k.to_string();
            map.insert("peer_id", value.as_str());
            peer.with(&map).inc_by(v as f64);
        }
        family.push(peer.collect()[0].clone());

        // protocol_in
        let opt = Opts::new("protocol_input_bytes", "Each protocol input bytes");
        let protocol = CounterVec::new(opt, &["protocol"]).unwrap();
        for (k, v) in self.control.protocol_in_iter() {
            let mut map = HashMap::new();
            let value = k.to_string();
            map.insert("protocol", value.as_str());
            protocol.with(&map).inc_by(v as f64);
        }
        family.push(protocol.collect()[0].clone());

        // protocol_out
        let opt = Opts::new("protocol_output_bytes", "Each protocol output bytes");
        let protocol = CounterVec::new(opt, &["protocol"]).unwrap();
        for (k, v) in self.control.protocol_out_iter() {
            let mut map = HashMap::new();
            let value = k.to_string();
            map.insert("protocol", value.as_str());
            protocol.with(&map).inc_by(v as f64);
        }
        family.push(protocol.collect()[0].clone());

        family
    }
}

impl Exporter {
    pub fn new(control: Control) -> Self {
        let d = Desc::new(
            "libp2prs_exporter".into(),
            "An Exporter that exposes metrics in libp2p_rs".into(),
            vec![],
            HashMap::new(),
        );
        Exporter { control, desc: d.unwrap() }
    }
}
