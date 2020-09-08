
use async_std::task;
use log;

use libp2p_core::{Multiaddr, Transport};
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::{TransportListener, TransportError};
use libp2p_traits::{Write2, Read2, ReadExt2, copy};

use secio;
use libp2p_core::identity::Keypair;
use secio::handshake::Config;
use libp2p_core::upgrade::{Multistream, DummyUpgrader, Upgrader, Selector};

fn main() {
    env_logger::init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();

    task::spawn( async move {

        log::info!("starting echo server...");

        let sec = Config::new(Keypair::generate_secp256k1());
        let mux = Selector::new(Config::new(Keypair::generate_secp256k1()), DummyUpgrader::new());
        let t1 = TransportUpgrade::new(MemoryTransport::default(), Multistream::new(mux), Multistream::new(sec));
        //let t1 = TransportUpgrade::new(MemoryTransport::default(), DummyUpgrader::default());
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let mut stream = listener.accept().await.unwrap();
            task::spawn(async move {

                let (rx, tx) = stream.split();

                copy(rx, tx).await.unwrap();


                // let mut msg = vec![0; 4096];
                // loop {
                //     let n = stream.read2(&mut msg).await?;
                //     stream.write2(&msg[..n]).await?;
                // }

                Ok::<(), std::io::Error>(())
            });
        }
    });

    // Setup dialer.
    task::block_on(async {
        for i in 0..1u32 {
            let addr = t1_addr.clone();
            task::spawn(async move {
                let mut msg = [1, 2, 3];
                log::info!("start client{}", i);

                let sec = Config::new(Keypair::generate_secp256k1());
                let mux = Selector::new(Config::new(Keypair::generate_secp256k1()), DummyUpgrader::new());
                let t2 = TransportUpgrade::new(MemoryTransport::default(), Multistream::new(mux), Multistream::new(sec));
                let mut socket = t2.dial(addr).await?;

                socket.write_all2(&msg).await.unwrap();
                socket.read_exact2(&mut msg).await.unwrap();
                log::info!("client{} got {:?}", i, msg);

                socket.close2().await?;
                Ok::<(), TransportError>(())
            }).await;
        }
    });
}