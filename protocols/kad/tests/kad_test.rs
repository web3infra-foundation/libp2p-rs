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

use libp2prs_core::identity::Keypair;
use libp2prs_core::multiaddr::protocol::Protocol;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_kad::store::MemoryStore;
use libp2prs_kad::{kad::Kademlia, Control as kad_control};
use libp2prs_plaintext as plaintext;
use libp2prs_runtime::task;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::{Control as swarm_control, Swarm};
use libp2prs_yamux as yamux;
use quickcheck::{QuickCheck, TestResult};
use rand::random;
use std::time::Duration;

fn setup_kad(keys: Keypair, listen_addr: Multiaddr) -> (swarm_control, kad_control) {
    let sec = plaintext::PlainTextConfig::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(MemoryTransport::default(), mux, sec);

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_identify(IdentifyConfig::new(false));

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // start kad protocol
    let store = MemoryStore::new(swarm.local_peer_id().clone());
    let kad = Kademlia::new(swarm.local_peer_id().clone(), store);
    let kad_handler = kad.handler();
    let kad_ctrl = kad.control();
    kad.start(swarm.control());

    // register handler to swarm
    swarm = swarm.with_protocol(Box::new(kad_handler)).with_routing(Box::new(kad_ctrl.clone()));

    swarm.listen_on(vec![listen_addr]).expect("listen on");
    let swarm_ctrl = swarm.control();
    swarm.start();

    (swarm_ctrl, kad_ctrl)
}

#[derive(Clone)]
struct PeerInfo {
    pid: PeerId,
    addr: Multiaddr,
    swarm_ctrl: swarm_control,
    kad_ctrl: kad_control,
}

fn setup_kads(n: usize) -> Vec<PeerInfo> {
    // Now swarm.close() do not close connection.
    // So use random port to protect from reuse port error
    let base_port = 1 + random::<u64>();
    let mut peers_info = vec![];
    for i in 0..n {
        let key = Keypair::generate_ed25519();
        let pid = key.public().into_peer_id();
        let addr: Multiaddr = Protocol::Memory(base_port + i as u64).into();

        let (swarm_ctrl, kad_ctrl) = setup_kad(key, addr.clone());
        peers_info.push(PeerInfo {
            pid,
            addr,
            swarm_ctrl,
            kad_ctrl,
        });
    }
    peers_info
}

async fn connect(a: &mut PeerInfo, b: &mut PeerInfo) {
    a.swarm_ctrl
        .connect_with_addrs(b.pid.clone(), vec![b.addr.clone()])
        .await
        .expect("connect");
}

#[test]
fn test_bootstrap() {
    fn prop() -> TestResult {
        task::block_on(async {
            let infos = setup_kads(3);
            let mut node0 = infos.get(0).expect("get peer info").clone();
            let mut node1 = infos.get(1).expect("get peer info").clone();
            let mut node2 = infos.get(2).expect("get peer info").clone();

            connect(&mut node0, &mut node1).await;
            connect(&mut node1, &mut node2).await;

            // wait for identify result
            task::sleep(Duration::from_millis(200)).await;

            // expect node2 find node0 via node1
            node0.swarm_ctrl.new_connection(node2.pid.clone()).await.expect("new connection");

            TestResult::passed()
        })
    }
    // env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
}

#[test]
fn test_simple_value_get_set() {
    fn prop() -> TestResult {
        task::block_on(async {
            let infos = setup_kads(5);
            let mut node0 = infos.get(0).expect("get peer info").clone();
            let mut node1 = infos.get(1).expect("get peer info").clone();
            let mut node2 = infos.get(2).expect("get peer info").clone();
            let mut node3 = infos.get(3).expect("get peer info").clone();
            let mut node4 = infos.get(4).expect("get peer info").clone();

            connect(&mut node0, &mut node1).await;
            connect(&mut node1, &mut node2).await;
            connect(&mut node2, &mut node3).await;
            connect(&mut node3, &mut node4).await;

            let key = b"/v/hello".to_vec();
            let value = b"world".to_vec();
            let _ = node0.kad_ctrl.put_value(key.clone(), value.clone()).await;

            // wait for identify result
            task::sleep(Duration::from_millis(200)).await;

            // expect node0<->node1<->node2<->node3<->node4
            // node4 and node3 need to iterative query to get value
            for i in 1..5 {
                let mut pi = infos.get(i).expect("get peer info").clone();
                let v = pi.kad_ctrl.get_value(key.clone()).await.unwrap();
                assert_eq!(value, v);
            }

            TestResult::passed()
        })
    }
    // env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
}

#[test]
fn test_simple_provides() {
    fn prop() -> TestResult {
        task::block_on(async {
            let infos = setup_kads(5);
            let mut node0 = infos.get(0).expect("get peer info").clone();
            let mut node1 = infos.get(1).expect("get peer info").clone();
            let mut node2 = infos.get(2).expect("get peer info").clone();
            let mut node3 = infos.get(3).expect("get peer info").clone();
            let mut node4 = infos.get(4).expect("get peer info").clone();

            connect(&mut node0, &mut node1).await;
            connect(&mut node1, &mut node2).await;
            connect(&mut node2, &mut node3).await;
            connect(&mut node3, &mut node4).await;

            // wait for identify result
            task::sleep(Duration::from_millis(200)).await;

            let keys: [&str; 3] = ["hello", "world", "rust"];
            for key in keys.iter().cloned() {
                let _ = node0.kad_ctrl.provide(Vec::from(key)).await;
            }

            // expect node0<->node1<->node2<->node3<->node4
            // node4 and node3 need to iterative query to get provider
            for i in 1..5 {
                let mut pi = infos.get(i).expect("get peer info").clone();
                for key in keys.iter().cloned() {
                    let providers = pi.kad_ctrl.find_providers(Vec::from(key), 1).await.expect("can't find provider");
                    for provider in providers {
                        assert_eq!(node0.pid, provider.node_id);
                        if let Some(addr) = provider.multiaddrs.get(0).cloned() {
                            assert_eq!(node0.addr, addr);
                        }
                    }
                }
            }

            TestResult::passed()
        })
    }
    // env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
}

#[test]
fn test_simple_find_peer() {
    fn prop() -> TestResult {
        task::block_on(async {
            let infos = setup_kads(5);
            let mut node0 = infos.get(0).expect("get peer info").clone();
            let mut node1 = infos.get(1).expect("get peer info").clone();
            let mut node2 = infos.get(2).expect("get peer info").clone();
            let mut node3 = infos.get(3).expect("get peer info").clone();
            let mut node4 = infos.get(4).expect("get peer info").clone();

            connect(&mut node0, &mut node1).await;
            connect(&mut node1, &mut node2).await;
            connect(&mut node2, &mut node3).await;
            connect(&mut node3, &mut node4).await;

            // wait for identify result
            task::sleep(Duration::from_millis(200)).await;

            // expect node0<->node1<->node2<->node3<->node4
            for i in 1..5 {
                let mut pi = infos.get(i).expect("get peer info").clone();
                let p = pi.kad_ctrl.find_peer(&node0.pid).await.expect("find peer");
                assert_eq!(p.node_id, node0.pid.clone());
            }

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
}
