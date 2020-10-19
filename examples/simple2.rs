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

use async_std::task;
use log::{error, info};

use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{Multiaddr, Transport};
use libp2prs_dns::DnsConfig;
use libp2prs_tcp::TcpConfig;

use libp2prs_core::identity::Keypair;

use libp2prs_core::upgrade::Selector;
use libp2prs_mplex as mplex;
use libp2prs_secio as secio;
use libp2prs_yamux as yamux;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

fn run_server() {
    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(Keypair::generate_secp256k1());
    //let sec = DummyUpgrader::new();
    //let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
    // let mux = yamux::Config::new();
    // let mux = mplex::Config::new();
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let mut tu = TransportUpgrade::new(DnsConfig::new(TcpConfig::default()), mux, sec);

    task::block_on(async move {
        let mut listener = tu.listen_on(listen_addr).unwrap();

        loop {
            let mut stream_muxer = listener.accept().await.unwrap();
            info!("server accept a new connection: {:?}", stream_muxer);

            if let Some(task) = stream_muxer.task() {
                task::spawn(task);
            }

            while let Ok(mut stream) = stream_muxer.accept_stream().await {
                task::spawn(async move {
                    info!("accepted new stream: {:?}", stream);
                    let mut buf = [0; 4096];

                    loop {
                        let n = match stream.read2(&mut buf).await {
                            Ok(num) => num,
                            Err(e) => {
                                error!("{:?} read failed: {:?}", stream, e);
                                return;
                            }
                        };
                        if n == 0 {
                            return;
                        }
                        // info!("{:?} read {:?}", stream, &buf[..n]);
                        if let Err(e) = stream.write_all2(buf[..n].as_ref()).await {
                            error!("{:?} write failed: {:?}", stream, e);
                            return;
                        };
                    }
                });
            }
        }
    });
}

fn run_client() {
    // let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let addr: Multiaddr = "/dns4/example.com/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(Keypair::generate_secp256k1());
    //let sec = DummyUpgrader::new();
    //let mux = Selector::new(yamux::Config::new(), Selector::new(yamux::Config::new(), yamux::Config::new()));
    // let mux = yamux::Config::new();
    // let mux = mplex::Config::new();
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let mut tu = TransportUpgrade::new(DnsConfig::new(TcpConfig::default()), mux, sec);

    task::block_on(async move {
        let mut stream_muxer = tu.dial(addr).await.expect("listener is started already");
        info!("open a new connection: {:?}", stream_muxer);

        if let Some(task) = stream_muxer.task() {
            task::spawn(task);
        }

        let mut handles = Vec::new();
        for _ in 0..2_u32 {
            let mut stream = stream_muxer.open_stream().await.unwrap();
            let handle = task::spawn(async move {
                info!("C: opened new stream {:?}", stream);

                let data = b"hello world";

                stream.write_all2(data.as_ref()).await.unwrap();
                info!("C: {:?}: wrote {} bytes", stream, data.len());

                let mut frame = vec![0; data.len()];
                stream.read_exact2(&mut frame).await.unwrap();
                info!("C: {:?}: read {:?}", stream, &frame);
                // assert_eq!(&data[..], &frame[..]);

                stream.close2().await.expect("close stream");

                // wait for stream to send and recv close frame
                // task::sleep(Duration::from_secs(1)).await;
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await;
        }

        stream_muxer.close().await.expect("close connection");

        info!("shutdown is completed");
    });
}
