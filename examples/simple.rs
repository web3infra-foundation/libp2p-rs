use async_std::task;
use log;

use libp2p_core::identity::Keypair;
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::{TransportError, TransportListener};
use libp2p_core::{Multiaddr, Transport};
use libp2p_traits::{copy, Read2, ReadExt2, Write2};
use libp2p_core::upgrade::{Selector};
use pnet::{PnetConfig, PreSharedKey};
use secio;
use std::time::Duration;
use yamux;

fn main() {
    //env_logger::init();

    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();

    task::spawn(async move {
        log::info!("starting echo server...");

        let sec = secio::Config::new(Keypair::generate_secp256k1());
        //let sec = DummyUpgrader::new();
        let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
        //let mux = yamux::Config::new();
        //let mux = mplex::Config::new();
        let psk = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683".parse::<PreSharedKey>().unwrap();
        let pnet = PnetConfig::new(Some(psk));
        let t1 = TransportUpgrade::new(MemoryTransport::default(), pnet, mux, sec);
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let mut stream_muxer = listener.accept().await.unwrap();

            log::info!("server accept a new connection: {:?}", stream_muxer);
            if let Some(task) = stream_muxer.task() {
                task::spawn(task);
            }

            // spawn a task for handling this connection/stream-muxer
            task::spawn(async move {
                loop {
                    if let Ok(stream) = stream_muxer.accept_stream().await {
                        log::info!("server accepted a new substream {:?}", stream);
                        task::spawn(async {
                            let (rx, tx) = stream.split2();
                            copy(rx, tx).await?;
                            Ok::<(), std::io::Error>(())
                        });
                    } else {
                        log::warn!("stream_muxer {:?} closed", stream_muxer);
                        break;
                    }
                }

                // let mut msg = vec![0; 4096];
                // loop {
                //     let n = stream.read2(&mut msg).await?;
                //     stream.write2(&msg[..n]).await?;
                // }

                //});
            });
        }
    });

    // Setup dialer.
    task::block_on(async {
        task::sleep(Duration::from_secs(1)).await;
        for i in 0..2u32 {
            log::info!("start client{}", i);

            let addr = t1_addr.clone();
            task::spawn(async move {
                let mut msg = [1, 2, 3];

                let sec = secio::Config::new(Keypair::generate_secp256k1());
                //let sec = DummyUpgrader::new();
                //let mux = yamux::Config::new();
                //let mux = mplex::Config::new();
                let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
                let psk ="/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683".parse::<PreSharedKey>().unwrap();
                let pnet=PnetConfig::new(Some(psk));
                let t2 = TransportUpgrade::new(MemoryTransport::default(),pnet, mux, sec);
                let mut stream_muxer = t2.dial(addr).await.expect("listener is started already");

                if let Some(task) = stream_muxer.task() {
                    task::spawn(task);
                }

                for j in 0..1u32 {
                    let mut socket = stream_muxer.open_stream().await?;

                    log::info!("client{}/{} got a new substream {:?}", i, j, socket);

                    socket.write_all2(&msg).await.unwrap();
                    socket.read_exact2(&mut msg).await.unwrap();
                    log::info!("client{}/{} got {:?}", i, j, msg);

                    socket.close2().await.unwrap();
                }

                stream_muxer.close().await.expect("close error");
                Ok::<(), TransportError>(())
            }).await.expect("error");

            log::info!("client{} exited", i);
        }
    });
}
