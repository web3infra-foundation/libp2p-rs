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

use log::{error, info};
#[macro_use]
extern crate lazy_static;

use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{
    multiaddr::protocol::Protocol,
    pnet::{PnetConfig, PreSharedKey},
    transport::protector::ProtectorTransport,
    Multiaddr, PeerId,
};
use libp2prs_runtime::task;
use libp2prs_tcp::TcpConfig;

use libp2prs_core::identity::Keypair;

use libp2prs_core::upgrade::Selector;
use libp2prs_floodsub::{floodsub::FloodSub, FloodsubConfig, Topic};
use libp2prs_mplex as mplex;
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::Swarm;
use libp2prs_yamux as yamux;

use futures::StreamExt;
use std::path::PathBuf;
use std::{env, error::Error, fs, path::Path, str::FromStr, time::Duration};

/// Get the current ipfs repo path, either from the IPFS_PATH environment variable or
/// from the default $HOME/.ipfs
fn get_ipfs_path() -> Box<Path> {
    env::var("IPFS_PATH")
        .map(|ipfs_path| Path::new(&ipfs_path).into())
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|home| Path::new(&home).join(".ipfs"))
                .expect("could not determine home directory")
                .into()
        })
}

/// Read the pre shared key file from the given ipfs directory
fn get_psk(swarm_key_file: PathBuf) -> std::io::Result<Option<String>> {
    match fs::read_to_string(swarm_key_file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// for a multiaddr that ends with a peer id, this strips this suffix. Rust-libp2p
/// only supports dialing to an address without providing the peer id.
fn strip_peer_id(addr: &mut Multiaddr) -> Option<PeerId> {
    let last = addr.pop();
    match last {
        Some(Protocol::P2p(peer_id)) => {
            let pid = PeerId::from_multihash(peer_id).expect("peerid from multihash");
            return Some(pid);
        }
        Some(other) => addr.push(other),
        _ => {}
    }
    None
}

/// parse a legacy multiaddr (replace ipfs with p2p), and strip the peer id
/// so it can be dialed by rust-libp2p
fn parse_legacy_multiaddr(text: &str) -> Result<Multiaddr, Box<dyn Error>> {
    let sanitized = text
        .split('/')
        .map(|part| if part == "ipfs" { "p2p" } else { part })
        .collect::<Vec<_>>()
        .join("/");
    let res = Multiaddr::from_str(&sanitized)?;
    Ok(res)
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
    static ref FLOODSUB_TOPIC: Topic = Topic::new("chat");
}

fn main() -> Result<(), Box<dyn Error>> {
    task::block_on(entry())
}

async fn entry() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/38087".parse().unwrap();

    let mut local_key = SERVER_KEY.clone();

    // Reach out to other nodes if specified
    let mut addr: Multiaddr = Multiaddr::empty();
    let mut remote_peer_id = PeerId::random();
    let mut dialer = false;
    if let Some(to_dial) = std::env::args().nth(1) {
        dialer = true;
        addr = parse_legacy_multiaddr(&to_dial)?;
        remote_peer_id = strip_peer_id(&mut addr).expect("lack of peer id");
        local_key = Keypair::generate_ed25519();
        log::info!("Dialed {:?}", to_dial);
    }

    let local_peer_id = local_key.public().into_peer_id();
    info!("using random peer id: {:?}", local_peer_id);
    info!("Address: {}/ipfs/{}", listen_addr, local_peer_id);

    let sec = secio::Config::new(local_key.clone());
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());

    // Get shared key
    let ipfs_path = get_ipfs_path();
    info!("using IPFS_PATH {:?}", ipfs_path);
    let swarm_key_file = ipfs_path.join("swarm.key");
    let psk: Option<PreSharedKey> = get_psk(swarm_key_file)?.map(|text| PreSharedKey::from_str(&text)).transpose()?;

    if let Some(psk) = psk {
        info!("using swarm key with fingerprint: {}", psk.fingerprint());
    }

    let psk = match psk {
        Some(psk) => psk,
        None => {
            error!("psk is empty");
            return Ok(());
        }
    };

    // Protector Transport
    let pnet = PnetConfig::new(psk);
    let tpt = ProtectorTransport::new(TcpConfig::default(), pnet);
    let tu = TransportUpgrade::new(tpt, mux, sec);

    let floodsub = FloodSub::new(FloodsubConfig::new(local_peer_id));
    let handler = floodsub.handler();

    let mut swarm = Swarm::new(local_key.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(true));

    // run floodsub mainloop
    let mut swarm_control = swarm.control();
    let floodsub_control = floodsub.control();
    floodsub.start(swarm_control.clone());

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    if !dialer {
        swarm.listen_on(vec![listen_addr]).unwrap();
    }

    swarm.start();

    task::block_on(async {
        // if dialer
        if dialer {
            swarm_control.connect_with_addrs(remote_peer_id, vec![addr]).await.unwrap();
        }

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
            let _ = std::io::stdin().read_line(&mut line);
            let x: &[_] = &['\r', '\n'];
            let msg = line.trim_end_matches(x);
            floodsub_control.clone().publish(Topic::new(FLOODSUB_TOPIC.clone()), msg).await;
        }
    });

    Ok(())
}
