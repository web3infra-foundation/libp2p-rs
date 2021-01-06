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

use libp2prs_core::identity;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::Selector;
use libp2prs_core::Multiaddr;
use libp2prs_kad::kad::Kademlia;
use libp2prs_kad::store::MemoryStore;
use libp2prs_kad::Control as KadControl;
use libp2prs_mplex as mplex;
use libp2prs_noise::{Keypair, NoiseConfig, X25519Spec};
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::Control as SwarmControl;
use libp2prs_swarm::Swarm;
use libp2prs_tcp::TcpConfig;
use libp2prs_yamux as yamux;

use libp2prs_kad::cli::dht_cli_commands;
use libp2prs_multiaddr::protocol::Protocol;
use libp2prs_swarm::cli::swarm_cli_commands;
use xcli::*;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let port = std::env::args().nth(1).expect("usage: ./kad_nodes.exe <listen_port>");
    let listen_port = port.parse::<u16>().expect("string to u16");

    let keys = identity::Keypair::generate_ed25519();
    let mut listen_addr: Multiaddr = "/ip4/0.0.0.0".parse().unwrap();
    listen_addr.push(Protocol::Tcp(listen_port));
    let (swarm_control, kad_control) = setup_kad(keys, listen_addr);

    let mut app = App::new("xCLI").version("v0.1").author("kingwel.xie@139.com");

    app.add_subcommand_with_userdata(swarm_cli_commands(), Box::new(swarm_control));
    app.add_subcommand_with_userdata(dht_cli_commands(), Box::new(kad_control));

    app.run();
}

fn setup_kad(keys: identity::Keypair, listen_addr: Multiaddr) -> (SwarmControl, KadControl) {
    // let sec = secio::Config::new(keys.clone());
    let dh = Keypair::<X25519Spec>::new().into_authentic(&keys).unwrap();
    let sec = NoiseConfig::xx(dh, keys.clone());
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

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
    swarm = swarm.with_protocol(Box::new(kad_handler));

    swarm.listen_on(vec![listen_addr.clone()]).expect("listen on");
    let swarm_ctrl = swarm.control();
    swarm.start();

    let pid = keys.public().into_peer_id();
    log::info!("I can be reached at: {}/p2p/{}", listen_addr, pid);

    (swarm_ctrl, kad_ctrl)
}
