use futures::executor::block_on;
use libp2prs_core::{
    identity::Keypair,
    multiaddr::Protocol,
    transport::{memory::MemoryTransport, upgrade::TransportUpgrade},
};
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_gossipsub::{
    control::Control as GossipControl,
    gossipsub::{Gossipsub, MessageAuthenticity},
    subscription_filter::AllowAllSubscriptionFilter,
    topic::IdentityHash,
    GossipsubConfigBuilder, IdentityTransform, Topic,
};
use libp2prs_runtime::task;
use libp2prs_secio as secio;
use libp2prs_swarm::{identify::IdentifyConfig, ping::PingConfig, Control as SwarmControl, Swarm};
use libp2prs_yamux as yamux;
use rand::random;
use std::time::Duration;

fn setup_swarm() -> (Swarm, GossipControl) {
    let key = Keypair::generate_ed25519();
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

#[derive(Clone)]
pub struct Node {
    pub swarm_control: SwarmControl,
    pub gossip_control: GossipControl,
    pub addr: Multiaddr,
    pub peer_id: PeerId,
}

impl Node {
    pub fn get_swarm(&self) -> SwarmControl {
        self.swarm_control.clone()
    }

    pub fn get_gossip(&self) -> GossipControl {
        self.gossip_control.clone()
    }

    pub fn get_addr(&self) -> Multiaddr {
        self.addr.clone()
    }
}

fn new_node(n: i32) -> Vec<Node> {
    let mut node_list = vec![];
    for _ in 0..n {
        let (mut swarm, gossip) = setup_swarm();
        let port = 1 + random::<u64>();
        let addr: Multiaddr = Protocol::Memory(port).into();
        let node = Node {
            swarm_control: swarm.control(),
            gossip_control: gossip,
            addr: addr.clone(),
            peer_id: swarm.local_peer_id().clone(),
        };
        let _ = swarm.listen_on(vec![addr]);
        swarm.start();
        node_list.push(node);
    }
    node_list
}

/// Each peer connected to
fn sparse_connect(v: Vec<Node>) {
    let node_length = v.len();
    let connected_node = 3;
    for (index, node) in v.iter().enumerate() {
        for mut i in 0..connected_node {
            let n = random::<usize>() % node_length;
            if n == index {
                i = i - 1;
                continue;
            }

            let remote_node = v.get(n).unwrap();
            block_on(async {
                let _ = node
                    .get_swarm()
                    .connect_with_addrs(remote_node.peer_id, vec![remote_node.get_addr()])
                    .await;
            })
        }
    }
}

fn dense_connect(v: Vec<Node>) {
    let connected_node = 10;
    for (index, node) in v.iter().enumerate() {
        for mut i in 0..connected_node {
            let n = random::<usize>() % v.len();
            if n == index {
                i = i - 1;
                continue;
            }

            let remote_node = v.get(n).unwrap();
            block_on(async {
                let _ = node
                    .get_swarm()
                    .connect_with_addrs(remote_node.peer_id, vec![remote_node.get_addr()])
                    .await;
            })
        }
    }
}

#[test]
pub fn test_gossip_fanout() {
    env_logger::init();
    // fn prop() -> TestResult {
    let topic: Topic<IdentityHash> = Topic::new("Hello World");
    let node_list = new_node(20);
    dense_connect(node_list.clone());
    let mut subscription_list = vec![];

    task::block_on(async {
        let message = b"foobar";

        for i in 1..node_list.len() {
            let mut gossip = node_list[i].get_gossip();
            let subscription = gossip.subscribe(topic.hash()).await.unwrap();
            subscription_list.push(subscription);
        }

        task::sleep(Duration::from_secs(2)).await;

        let _ = node_list[0].get_gossip().publish(topic.hash(), message.to_vec()).await;

        for mut subscription in subscription_list {
            match subscription.next().await {
                Some(msg) => {
                    assert_eq!(msg.data, message.to_vec());
                }
                None => {}
            }
        }
    })
    // }
    // QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
pub fn test_gossip_fanout_maintenance() {
    let topic: Topic<IdentityHash> = Topic::new("Hello World");
    let node_list = new_node(20);
    let mut subscription_list = vec![];

    task::block_on(async {
        // Subscribe and publish message from a fanout node.
        let message = b"foobar";

        for i in 1..node_list.len() {
            let mut gossip = node_list[i].get_gossip();
            let subscription = gossip.subscribe(topic.hash()).await.unwrap();
            subscription_list.push(subscription);
        }

        dense_connect(node_list.clone());

        task::sleep(Duration::from_secs(2)).await;

        let _ = node_list[0].get_gossip().publish(topic.hash(), message.to_vec()).await;

        for subscription in subscription_list.iter_mut() {
            match subscription.next().await {
                Some(msg) => {
                    assert_eq!(msg.data, message.to_vec());
                }
                None => {
                    unreachable!()
                }
            }
        }

        for node in node_list.iter() {
            let gossip = node.get_gossip();
            gossip.unsubscribe(topic.hash()).await;
        }

        task::sleep(Duration::from_secs(2)).await;

        subscription_list.clear();

        for i in 1..node_list.len() {
            let mut gossip = node_list[i].get_gossip();
            let subscription = gossip.subscribe(topic.hash()).await.unwrap();
            subscription_list.push(subscription);
        }

        task::sleep(Duration::from_secs(2)).await;

        let _ = node_list[0].get_gossip().publish(topic.hash(), message.to_vec()).await;

        for mut subscription in subscription_list {
            match subscription.next().await {
                Some(msg) => {
                    assert_eq!(msg.data, message.to_vec());
                }
                None => {}
            }
        }
    });
}
