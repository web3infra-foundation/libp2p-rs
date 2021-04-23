use libp2prs_runtime::task;

use libp2prs_core::identity::Keypair;
use libp2prs_core::Multiaddr;
use libp2prs_mdns::service::MdnsService;
use libp2prs_mdns::{AddrInfo, MdnsConfig, Notifee};
use std::time::Duration;

pub struct DiscoveryNotifee {}

impl Notifee for DiscoveryNotifee {
    fn handle_peer_found(&mut self, peer: AddrInfo) {
        log::info!("Discovered peer {}", peer);
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    task::block_on(async {
        let pid = Keypair::generate_ed25519().public().into_peer_id();
        let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().unwrap();
        let config = MdnsConfig::new(pid, vec![listen_addr], false);
        let service = MdnsService::new(config);
        let mut control = service.control();

        service.start();
        control.register_notifee(Box::new(DiscoveryNotifee {})).await;

        loop {
            task::sleep(Duration::from_secs(120)).await;
        }
    });
}
