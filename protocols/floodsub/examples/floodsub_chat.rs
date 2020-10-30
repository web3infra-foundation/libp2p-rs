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

use async_std::{io, task};
use std::time::Duration;
#[macro_use]
extern crate lazy_static;

use futures::StreamExt;
use libp2prs_core::identity::Keypair;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_floodsub::{control::Control as Floodsub_Config, floodsub::FloodSub, FloodsubConfig, Topic};
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::Swarm;
use libp2prs_tcp::TcpConfig;
use libp2prs_yamux as yamux;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        log::info!("Starting server ......");
        run_server();
    } else {
        log::info!("Starting client ......");
        run_client();
    }
}

fn setup_swarm(keys: Keypair) -> (Swarm, Floodsub_Config) {
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let local_peer_id = keys.public().into_peer_id();
    let floodsub = FloodSub::new(FloodsubConfig::new(local_peer_id));
    let handler = floodsub.handler();

    let swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    // run floodsub message process main loop
    let floodsub_control = floodsub.control();
    floodsub.start(swarm.control());

    (swarm, floodsub_control)
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
    static ref FLOODSUB_TOPIC: Topic = Topic::new("chat");
}

#[allow(clippy::empty_loop)]
fn run_server() {
    let keys = SERVER_KEY.clone();

    let (mut swarm, floodsub_control) = setup_swarm(keys);

    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    swarm.listen_on(vec![listen_addr]).unwrap();

    swarm.start();

    task::block_on(async {
        // subscribe "chat"
        let mut control = floodsub_control.clone();
        task::spawn(async move {
            let sub = control.subscribe(FLOODSUB_TOPIC.clone()).await;
            if let Some(mut sub) = sub {
                loop {
                    if let Some(msg) = sub.ch.next().await {
                        log::info!("recived: {:?}", msg.data)
                    }
                }
            }
        });

        // publish
        loop {
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line).await;
            let x: &[_] = &['\r', '\n'];
            let msg = line.trim_end_matches(x);
            floodsub_control.clone().publish(Topic::new(FLOODSUB_TOPIC.clone()), msg).await;
        }
    });
}

fn run_client() {
    let keys = Keypair::generate_secp256k1();

    let (mut swarm, floodsub_control) = setup_swarm(keys);
    let mut swarm_control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.peer_addrs_add(&remote_peer_id, "/ip4/127.0.0.1/tcp/8086".parse().unwrap(), Duration::default());

    swarm.start();

    task::block_on(async {
        // dial
        swarm_control.new_connection(remote_peer_id.clone()).await.unwrap();

        // subscribe "chat"
        let mut control = floodsub_control.clone();
        task::spawn(async move {
            let sub = control.subscribe(FLOODSUB_TOPIC.clone()).await;
            if let Some(mut sub) = sub {
                loop {
                    if let Some(msg) = sub.ch.next().await {
                        log::info!("recived: {:?}", msg.data)
                    }
                }
            }
        });

        // publish
        loop {
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line).await;
            let x: &[_] = &['\r', '\n'];
            let msg = line.trim_end_matches(x);
            floodsub_control.clone().publish(Topic::new(FLOODSUB_TOPIC.clone()), msg).await;
        }
    });
}
