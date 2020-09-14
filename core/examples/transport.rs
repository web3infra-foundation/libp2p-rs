use async_std::task;
use log;

use futures::{AsyncReadExt, AsyncWriteExt};
use libp2p_core::pnet_core::*;
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::TransportListener;
use libp2p_core::upgrade::DummyUpgrader;
use libp2p_core::{Multiaddr, Transport};
use libp2p_traits::Write2;

fn main() {
    env_logger::init();

    let msg = [1, 2, 3];

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();

    task::spawn(async move {
        let t1 = TransportUpgrade::new(
            MemoryTransport::default(),
            DummyPnet::new(),
            DummyUpgrader::new(),
            DummyUpgrader::new(),
        );
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
                MemoryTransport::default(),
                DummyPnet::new(),
                DummyUpgrader::new(),
                DummyUpgrader::new(),
            );
            let mut socket = t2.dial(addr).await.unwrap();

            let mut msg = [1, 2, 3];
            socket.write_all(&msg).await.unwrap();
            //socket.read_exact(&mut msg).await.unwrap();
            log::info!("client{} got {:?}", i, msg);
        });
    }

    loop {}
}
