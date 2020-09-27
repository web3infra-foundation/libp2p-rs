use async_std::task;
use async_trait::async_trait;
use std::time::Duration;
#[macro_use]
extern crate lazy_static;

use libp2p_traits::{Read2, Write2};
use libp2p_core::identity::Keypair;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{Swarm, DummyProtocolHandler, Muxer, SwarmError};
use libp2p_tcp::TcpConfig;
use secio;
use yamux;
use libp2p_swarm::protocol_handler::{ProtocolHandler, BoxHandler};
use libp2p_swarm::ping::{PingConfig};
use libp2p_core::upgrade::UpgradeInfo;
use libp2p_swarm::identify::IdentifyConfig;


//use libp2p_swarm::Swarm::network::NetworkConfig;

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

    #[derive(Clone)]
    struct MyProtocolHandler;

    impl UpgradeInfo for MyProtocolHandler {
        type Info = &'static [u8];

        fn protocol_info(&self) -> Vec<Self::Info> {
            vec![b"/my/1.0.0"]
        }
    }

    #[async_trait]
    impl<C> ProtocolHandler<C> for MyProtocolHandler
    where
        C: Read2 + Write2 + Unpin + Send + std::fmt::Debug + 'static
    {
        async fn handle(&mut self, stream: C, info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
            let mut stream = stream;
            log::trace!("MyProtocolHandler handling inbound {:?}", stream);
            let mut msg = vec![0; 4096];
            loop {
                let n = stream.read2(&mut msg).await?;
                log::info!("received: {:?}", &msg[..n]);
                stream.write2(&msg[..n]).await?;
            }
        }

        fn box_clone(&self) -> BoxHandler<C> {
            Box::new(self.clone())
        }
    }

    let mut muxer = Muxer::new();
    let dummy_handler = Box::new(DummyProtocolHandler::new());
    muxer.add_protocol_handler(dummy_handler);
    muxer.add_protocol_handler(Box::new(MyProtocolHandler));

    let mut swarm = Swarm::new(tu, PeerId::from_public_key(keys.public()), muxer)
        .with_ping(PingConfig::new().with_unsolicited(false).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig);


    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

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

    let mut muxer = Muxer::new();
    // let dummy_handler = Box::new(DummyProtocolHandler::new());
    // muxer.add_protocol_handler(dummy_handler);

    let mut swarm = Swarm::new(tu,PeerId::from_public_key(keys.public()), muxer)
        .with_ping(PingConfig::new().with_unsolicited(false).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig);


    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.peers.addrs.add_addr(
        &remote_peer_id,
        "/ip4/127.0.0.1/tcp/8086".parse().unwrap(),
        Duration::default(),
    );

    swarm.start();

    task::block_on(async move {
        control.new_connection(remote_peer_id.clone()).await.unwrap();
        let mut stream = control.new_stream(remote_peer_id, vec!(b"/my/1.0.0")).await.unwrap();

        log::info!("stream {:?} opened, writing something...", stream);

        stream.write_all2(b"hello").await;

        task::sleep(Duration::from_secs(40)).await;

        control.close_stream(stream).await.unwrap();


        log::info!("shutdown is completed");
    });
}
