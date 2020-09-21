use async_std::task;
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::protector::ProtectorTransport;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::{TransportListener};
use libp2p_core::upgrade::DummyUpgrader;
use libp2p_core::{Multiaddr, Transport};
use libp2p_traits::{Read2, Write2};
use log;
use libp2p_core::pnet::{PnetConfig, PreSharedKey};


fn main() {
    env_logger::init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();
    let psk ="/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683".parse::<PreSharedKey>().unwrap();
    let pnet = PnetConfig::new(psk);
    //let pro_trans = ProtectorTransport::new(MemoryTransport::default(), pnet);
    let pro_trans = MemoryTransport::default();
    task::spawn(async move {
        let t1 = TransportUpgrade::new(pro_trans, DummyUpgrader::new(), DummyUpgrader::new());
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let mut stream = listener.accept().await.unwrap();
            task::spawn(async move {
                stream.write2(b"hello").await?;
                Ok::<(), std::io::Error>(())
            });
        }
    });

    // Setup dialer.
    for i in 0..10u32 {
        let addr = t1_addr.clone();
        task::spawn(async move {
            log::info!("start client{}", i);

            let t2 = TransportUpgrade::new(
                pro_trans,
                DummyUpgrader::new(),
                DummyUpgrader::new(),
            );
            let mut socket = t2.dial(addr).await.unwrap();

            let mut msg = [0; 5];
            //socket.write_all2(&msg).await.unwrap();
            socket.read_exact2(&mut msg).await.unwrap();
            log::info!("client{} got {:?}", i, String::from_utf8_lossy(&mut msg));
        });
    }

    loop {}
}
