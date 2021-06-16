#[macro_use]
extern crate lazy_static;

use crate::gossip::topic::IdentityHash;
use libp2prs_core::identity::Keypair;
use libp2prs_core::multiaddr::Protocol;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_gossipsub as gossip;
use libp2prs_gossipsub::control::Control;
use libp2prs_gossipsub::gossipsub::{Gossipsub, MessageAuthenticity};
use libp2prs_gossipsub::subscription_filter::AllowAllSubscriptionFilter;
use libp2prs_gossipsub::{GossipsubConfigBuilder, IdentityTransform, Topic};
use libp2prs_runtime::task;
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::Swarm;
use libp2prs_yamux as yamux;
use quickcheck::{QuickCheck, TestResult};
use rand::random;
use std::time::Duration;

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
    static ref LISTEN_ADDRESS: Multiaddr = Protocol::Memory(8085).into();
    static ref GOSSIP_TOPIC: Topic<IdentityHash> = Topic::new("test");
}

fn setup_swarm(key: Keypair) -> (Swarm, Control) {
    let sec = secio::Config::new(key.clone());
    let yamux = yamux::Config::new();
    let t = TransportUpgrade::new(MemoryTransport::default(), yamux, sec);

    let gossip_config = GossipsubConfigBuilder::new(key.public().into_peer_id()).build().unwrap();
    let gossip =
        Gossipsub::<IdentityTransform, AllowAllSubscriptionFilter>::new(MessageAuthenticity::Signed(key.clone()), gossip_config)
            .unwrap();
    let gossip_control = gossip.control();

    let s = Swarm::new(key.public())
        .with_transport(Box::new(t))
        .with_protocol(gossip)
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    (s, gossip_control)
}

#[test]
fn test_gossipsub_basic() {
    env_logger::builder().init();
    fn prop() -> TestResult {
        task::block_on(async {
            let srv_keys = SERVER_KEY.clone();
            let (mut srv_swarm, mut srv_fs_ctrl) = setup_swarm(srv_keys);

            let port = 1 + random::<u64>();
            let addr: Multiaddr = Protocol::Memory(port).into();
            srv_swarm.listen_on(vec![addr.clone()]).unwrap();
            srv_swarm.start();

            let _ = srv_fs_ctrl.subscribe(GOSSIP_TOPIC.hash()).await;

            // client 1
            let cli_keys = Keypair::generate_secp256k1();
            let (cli_swarm, mut cli_gs_ctrl) = setup_swarm(cli_keys);
            let mut cli_swarm_ctrl = cli_swarm.control();

            let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());
            log::info!("about to connect to {:?}", remote_peer_id);

            cli_swarm.start();

            cli_swarm_ctrl.connect_with_addrs(remote_peer_id, vec![addr.clone()]).await.unwrap();

            let mut sub = cli_gs_ctrl.subscribe(GOSSIP_TOPIC.hash()).await.unwrap();

            let cli_handle = task::spawn(async move {
                match sub.next().await {
                    Some(msg) => {
                        log::info!("server received: {:?}", msg.data);
                        msg.data.clone()
                    }
                    None => Vec::<u8>::new(),
                }
            });

            // client 2
            let cli2_keys = Keypair::generate_ed25519();
            let cli2_public_key = cli2_keys.public();
            let (mut cli2_swarm, cli2_gs_control) = setup_swarm(cli2_keys);
            let mut cli2_swarm_control = cli2_swarm.control();

            let adr: Multiaddr = Protocol::Memory(1 + random::<u64>()).into();
            cli2_swarm.listen_on(vec![adr.clone()]).unwrap();

            cli2_swarm.start();

            cli2_swarm_control
                .connect_with_addrs(remote_peer_id, vec![addr.clone()])
                .await
                .unwrap();
            let mut c = cli2_gs_control.clone();
            let cli2_handle = task::spawn(async move {
                task::sleep(Duration::from_secs(1)).await;
                c.subscribe(GOSSIP_TOPIC.hash()).await.unwrap();
                // cli2_gs_control
                //     .clone()
                //     .publish(GOSSIP_TOPIC.hash(), message.to_vec())
                //     .await
                //     .unwrap();
            });
            cli2_handle.await;

            // client 3
            let cli3_keys = Keypair::generate_ed25519();
            let (cli3_swarm, cli3_gs_control) = setup_swarm(cli3_keys);
            let mut cli3_swarm_control = cli3_swarm.control();

            cli3_swarm.start();

            let mut c = cli3_gs_control.clone();
            let cli3_handle = task::spawn(async move {
                cli3_swarm_control
                    .connect_with_addrs(cli2_public_key.into_peer_id(), vec![adr])
                    .await
                    .unwrap();

                task::sleep(Duration::from_secs(1)).await;

                c.subscribe(GOSSIP_TOPIC.hash()).await.unwrap();

                task::sleep(Duration::from_secs(1)).await;
                // cli3_gs_control
                //     .clone()
                //     .publish(GOSSIP_TOPIC.hash(), message.to_vec())
                //     .await
                //     .unwrap();
            });
            cli3_handle.await;

            task::spawn(async move {
                task::sleep(Duration::from_secs(5)).await;
                cli_gs_ctrl.unsubscribe(GOSSIP_TOPIC.hash()).await;
            });

            let data = cli_handle.await.unwrap();
            let v = Vec::<u8>::new();
            if data.eq(&v) {
                TestResult::passed()
            } else {
                TestResult::failed()
            }

            // cli_gs_ctrl.un
        })
    }

    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _);
}
