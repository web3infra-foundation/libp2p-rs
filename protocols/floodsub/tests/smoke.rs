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

use async_std::task;
use std::time::Duration;
extern crate lazy_static;

use futures::{channel::mpsc, future, StreamExt};
use libp2prs_core::identity::Keypair;
use libp2prs_core::multiaddr::protocol::Protocol;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_floodsub::{control::Control as Floodsub_Control, floodsub::FloodSub, FloodsubConfig, Topic};
use libp2prs_mplex as mplex;
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::Swarm;
use quickcheck::{QuickCheck, TestResult};
use rand::prelude::SliceRandom;
use rand::{random, SeedableRng};

fn build_node() -> (Swarm, PeerId, Multiaddr, Floodsub_Control) {
    let keys = Keypair::generate_ed25519();
    let public_key = keys.public();
    let sec = secio::Config::new(keys);
    let mux = mplex::Config::new();
    let tu = TransportUpgrade::new(MemoryTransport::default(), mux, sec);

    let local_peer_id = public_key.clone().into_peer_id();
    let floodsub = FloodSub::new(FloodsubConfig::new(local_peer_id));
    let handler = floodsub.handler();

    let mut swarm = Swarm::new(public_key)
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    // run floodsub message process main loop
    let floodsub_control = floodsub.control();
    floodsub.start(swarm.control());

    let port = 1 + random::<u64>();
    let addr: Multiaddr = Protocol::Memory(port).into();
    swarm.listen_on(vec![addr.clone()]).unwrap();

    let lpid = swarm.local_peer_id().clone();
    (swarm, lpid, addr, floodsub_control)
}

struct Graph {
    pub ctrls: Vec<Floodsub_Control>,
}

impl Graph {
    async fn new_connected(num_nodes: usize, seed: u64) -> Graph {
        if num_nodes == 0 {
            panic!("expecting at least one node");
        }

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut not_connected_nodes =
            std::iter::once(())
                .cycle()
                .take(num_nodes)
                .map(|_| build_node())
                .collect::<Vec<(Swarm, PeerId, Multiaddr, Floodsub_Control)>>();

        let connected_node = not_connected_nodes.pop().unwrap();
        let lpid = connected_node.1.clone();
        let addr = connected_node.2.clone();
        let floodsub_ctrl = connected_node.3.clone();

        // start swarm
        connected_node.0.start();

        // connect other node need peerID, address.
        // floodsub pubsub need flood control
        let mut connected_nodes = vec![(lpid, addr, floodsub_ctrl)];

        while !not_connected_nodes.is_empty() {
            connected_nodes.shuffle(&mut rng);
            not_connected_nodes.shuffle(&mut rng);

            let mut next = not_connected_nodes.pop().unwrap();
            let connected_pid = connected_nodes[0].0.clone();
            let connected_addr = connected_nodes[0].1.clone();

            log::info!("Connect: {} -> {}", next.2.clone().pop().unwrap(), connected_addr);

            let mut swarm_ctrl = next.0.control();
            let lpid = next.1.clone();
            let addr = next.2.clone();
            let floodsub_ctrl = next.3.clone();

            next.0.peer_addrs_add(&connected_pid, connected_addr, Duration::default());
            next.0.start();

            swarm_ctrl.new_connection(connected_pid).await.unwrap();

            connected_nodes.push((lpid, addr, floodsub_ctrl));
        }

        let mut ctrls = Vec::new();
        for (_, _, ctrl) in &connected_nodes {
            ctrls.push(ctrl.clone());
        }

        Graph { ctrls }
    }
}

#[test]
fn multi_hop_propagation() {
    fn prop(num_nodes: u8, seed: u64) -> TestResult {
        task::block_on(async {
            if num_nodes < 2 || num_nodes > 100 {
                return TestResult::discard();
            }

            let message = b"Hello World";

            log::info!("number nodes: {:?}, seed: {:?}", num_nodes, seed);

            let mut graph = Graph::new_connected(num_nodes as usize, seed).await;
            let number_nodes = graph.ctrls.len();

            // Subscribe each node to the same topic.
            let (tx, rx) = mpsc::unbounded();
            let topic = Topic::new("test-net");
            for ctrl in &mut graph.ctrls {
                let tx = tx.clone();
                let mut sub = ctrl.subscribe(topic.clone()).await.unwrap();
                task::spawn(async move {
                    loop {
                        if let Some(msg) = sub.ch.next().await {
                            tx.unbounded_send(msg.data.len()).expect("unbounded_send");
                        }
                    }
                });
            }

            // wait for subscription announce
            task::sleep(Duration::from_secs(1)).await;

            // publish
            graph.ctrls[0].publish(Topic::new(topic.clone()), message.to_vec()).await;

            let n = rx.take(number_nodes - 1).fold(0, |acc, n| future::ready(acc + n)).await;
            if n == (number_nodes - 1) * message.len() {
                TestResult::passed()
            } else {
                TestResult::failed()
            }
        })
    }
    QuickCheck::new().tests(5).quickcheck(prop as fn(u8, u64) -> _);
}
