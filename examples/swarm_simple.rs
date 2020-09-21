use async_std::task;
use log::{info};
use std::time::Duration;
#[macro_use]
extern crate lazy_static;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::Swarm;
use libp2p_tcp::TcpConfig;
use libp2p_core::identity::Keypair;
use secio;
use yamux;

//use libp2p_swarm::Swarm::network::NetworkConfig;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
}

fn run_server() {
    let keys = SERVER_KEY.clone();

    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    // let mux = mplex::Config::new();
    //let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(tu, 100, PeerId::from_public_key(keys.public()));

    info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    let _control = swarm.control();

    swarm.listen_on(listen_addr).unwrap();

    swarm.start();

    loop {}
}

fn run_client() {
    let keys = Keypair::generate_secp256k1();

    let _addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    // let mux = mplex::Config::new();
    //let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(tu, 100, PeerId::from_public_key(keys.public()));
    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    info!("about to connect to {:?}", remote_peer_id);

    swarm.peers.addrs.add_addr(
        &remote_peer_id,
        "/ip4/127.0.0.1/tcp/8086".parse().unwrap(),
        Duration::default(),
    );

    swarm.start();

    task::block_on(async move {
        control.new_connection(&remote_peer_id).await.unwrap();
        let _stream = control.new_stream(&remote_peer_id).await.unwrap();

        info!("shutdown is completed");
    });
}
