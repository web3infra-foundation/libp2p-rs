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
#[macro_use]
extern crate lazy_static;

use futures::StreamExt;
use libp2prs_core::identity::Keypair;
use libp2prs_core::multiaddr::protocol::Protocol;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_floodsub::{control::Control as Floodsub_Control, floodsub::FloodSub, FloodsubConfig, Topic};
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::Swarm;
use libp2prs_yamux as yamux;
use quickcheck::{QuickCheck, TestResult};
use rand::random;

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
    static ref LISTEN_ADDRESS: Multiaddr = Protocol::Memory(8085).into();
    static ref FLOODSUB_TOPIC: Topic = Topic::new("test");
}

fn setup_swarm(keys: Keypair) -> (Swarm, Floodsub_Control) {
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(MemoryTransport::default(), mux, sec);

    let local_peer_id = keys.public().into_peer_id();
    let floodsub = FloodSub::new(FloodsubConfig::new(local_peer_id));
    let handler = floodsub.handler();

    let swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    // log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // run floodsub message process main loop
    let floodsub_control = floodsub.control();
    floodsub.start(swarm.control());

    (swarm, floodsub_control)
}

#[test]
fn test_floodsub_basic() {
    fn prop() -> TestResult {
        task::block_on(async {
            let message = b"Hello World";
            let srv_keys = SERVER_KEY.clone();
            let (mut srv_swarm, mut srv_fs_ctrl) = setup_swarm(srv_keys);

            let port = 1 + random::<u64>();
            let addr: Multiaddr = Protocol::Memory(port).into();
            srv_swarm.listen_on(vec![addr.clone()]).unwrap();
            srv_swarm.start();

            let mut sub = srv_fs_ctrl.subscribe(FLOODSUB_TOPIC.clone()).await.unwrap();
            let srv_handle = task::spawn(async move {
                // subscribe "chat"
                match sub.ch.next().await {
                    Some(msg) => {
                        log::info!("server recived: {:?}", msg.data);
                        msg.data
                    }
                    None => Vec::<u8>::new(),
                }
            });

            let cli_keys = Keypair::generate_secp256k1();
            let (mut cli_swarm, cli_fs_ctrl) = setup_swarm(cli_keys);
            let mut cli_swarm_ctrl = cli_swarm.control();

            let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());
            log::info!("about to connect to {:?}", remote_peer_id);

            cli_swarm.peer_addrs_add(&remote_peer_id, addr, Duration::default());
            cli_swarm.start();

            let cli_handle = task::spawn(async move {
                // dial
                cli_swarm_ctrl.new_connection(remote_peer_id).await.unwrap();

                // wait for connection
                task::sleep(Duration::from_secs(1)).await;

                // publish
                cli_fs_ctrl
                    .clone()
                    .publish(Topic::new(FLOODSUB_TOPIC.clone()), message.to_vec())
                    .await;
            });

            let data = srv_handle.await;
            cli_handle.await;
            if data.eq(message) {
                TestResult::passed()
            } else {
                TestResult::failed()
            }
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
}
