
use async_std::task;
use log;

use libp2p_core::{Multiaddr, Transport};
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::{TransportListener, TransportError};
use libp2p_traits::{Write2, Read2, ReadExt2, copy};

use secio;
use yamux;
use libp2p_core::identity::Keypair;
use libp2p_core::upgrade::{DummyUpgrader, Selector};
use libp2p_core::muxing::StreamMuxer;
use futures::StreamExt;
use futures::future;
use winapi::_core::time::Duration;

fn main() {
    env_logger::init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();

    task::spawn( async move {

        log::info!("starting echo server...");

        let sec = secio::Config::new(Keypair::generate_secp256k1());
        //let sec = DummyUpgrader::new();
        //let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
        let mux = yamux::Config::new();
        let t1 = TransportUpgrade::new(MemoryTransport::default(), mux, sec);
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let mut stream_muxer = listener.accept().await.unwrap();
            task::spawn(async move {

                loop {
                    let stream = stream_muxer.accept_stream().await.unwrap();
                    log::info!("server accepted a new substream {:?}", stream);

                    task::spawn(async {
                        let (rx, tx) = stream.split();
                        copy(rx, tx).await?;
                        Ok::<(), std::io::Error>(())
                    });
                }

                // let mut msg = vec![0; 4096];
                // loop {
                //     let n = stream.read2(&mut msg).await?;
                //     stream.write2(&msg[..n]).await?;
                // }

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

                let sec = secio::Config::new(Keypair::generate_secp256k1());
                //let sec = DummyUpgrader::new();
                let mux = yamux::Config::new();
                //let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
                let t2 = TransportUpgrade::new(MemoryTransport::default(), mux, sec);
                let mut stream_muxer = t2.dial(addr).await?;

                task::spawn(stream_muxer.take_inner_stream().unwrap().for_each(|_| future::ready(())));

                let mut socket = stream_muxer.open_stream().await?;

                log::info!("client{} got a new substream {:?}", i, socket);

                socket.write_all2(&msg).await.unwrap();
                socket.read_exact2(&mut msg).await.unwrap();
                log::info!("client{} got {:?}", i, msg);

                socket.close2().await?;
                Ok::<(), TransportError>(())
            }).await;
        }
    });
}