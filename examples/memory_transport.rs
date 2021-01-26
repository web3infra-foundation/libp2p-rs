// Copyright 2020 Netwarps Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use libp2prs_core::identity::Keypair;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::transport::ListenerEvent;
use libp2prs_core::upgrade::Selector;
use libp2prs_core::{Multiaddr, Transport};
use libp2prs_runtime::task;
use libp2prs_secio as secio;
use libp2prs_yamux as yamux;
use std::time::Duration;

fn main() {
    task::block_on(entry())
}

async fn entry() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();

    task::spawn(async move {
        log::info!("starting echo server...");

        let sec = secio::Config::new(Keypair::generate_secp256k1());
        //let sec = DummyUpgrader::new();
        let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
        let mut t1 = TransportUpgrade::new(MemoryTransport::default(), mux, sec);
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let event = listener.accept().await.unwrap();
            match event {
                ListenerEvent::Accepted(mut stream_muxer) => {
                    log::info!("server accept a new connection: {:?}", stream_muxer);
                    if let Some(task) = stream_muxer.task() {
                        task::spawn(task);
                    }

                    // spawn a runtime for handling this connection/stream-muxer
                    task::spawn(async move {
                        loop {
                            if let Ok(stream) = stream_muxer.accept_stream().await {
                                log::info!("server accepted a new substream {:?}", stream);
                                let mut stream_r = stream.clone();
                                let mut stream_w = stream.clone();
                                task::spawn(async move {
                                    let mut buf = [0; 4096];
                                    let n = stream_r.read2(&mut buf).await.unwrap();
                                    let _ = stream_w.write_all2(&buf[0..n]).await;
                                });
                            } else {
                                log::warn!("stream_muxer {:?} closed", stream_muxer);
                                break;
                            }
                        }
                    });
                }
                ListenerEvent::AddressAdded(addr) => {
                    log::info!("new address : {}", addr);
                }
                ListenerEvent::AddressDeleted(_) => {}
            }
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
                let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
                let mut t2 = TransportUpgrade::new(MemoryTransport::default(), mux, sec);
                let mut stream_muxer = t2.dial(addr).await.expect("listener is started already");

                if let Some(task) = stream_muxer.task() {
                    task::spawn(task);
                }

                for j in 0..1u32 {
                    let mut socket = stream_muxer.open_stream().await.unwrap();

                    log::info!("client{}/{} got a new substream {:?}", i, j, socket);

                    socket.write_all2(&msg).await.unwrap();
                    socket.read_exact2(&mut msg).await.unwrap();
                    log::info!("client{}/{} got {:?}", i, j, msg);

                    socket.close2().await.unwrap();
                }

                stream_muxer.close().await.expect("close error");
            })
            .await;

            log::info!("client{} exited", i);
        }
    });
}
