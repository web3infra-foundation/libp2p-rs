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

use futures::{AsyncReadExt, AsyncWriteExt};
use libp2p_pnet::{PnetConfig, PreSharedKey};
use log::{error, info};

use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::{transport::protector::ProtectorTransport, Multiaddr, Transport};
use libp2prs_dns::DnsConfig;
use libp2prs_runtime::task;
use libp2prs_tcp::TcpConfig;

use libp2prs_core::identity::Keypair;

use libp2prs_core::transport::ListenerEvent;
use libp2prs_core::upgrade::Selector;
use libp2prs_mplex as mplex;
use libp2prs_secio as secio;
use libp2prs_yamux as yamux;

fn main() {
    task::block_on(entry())
}

async fn entry() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server().await;
    } else {
        info!("Starting client ......");
        run_client().await;
    }
}

fn build_pet_config() -> ProtectorTransport<DnsConfig<TcpConfig>> {
    let psk = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683"
        .parse::<PreSharedKey>()
        .unwrap();
    let pnet = PnetConfig::new(psk);
    ProtectorTransport::new(DnsConfig::new(TcpConfig::default()), pnet)
}

async fn run_server() {
    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/38087".parse().unwrap();
    let sec = secio::Config::new(Keypair::generate_secp256k1());
    let mux = Selector::new(yamux::Config::server(), mplex::Config::new());
    let mut tu = TransportUpgrade::new(build_pet_config(), mux, sec);

    let mut listener = tu.listen_on(listen_addr).unwrap();

    loop {
        let mut stream_muxer = match listener.accept().await.unwrap() {
            ListenerEvent::Accepted(s) => s,
            _ => continue,
        };
        info!("server accept a new connection: {:?}", stream_muxer);

        if let Some(task) = stream_muxer.task() {
            task::spawn(task);
        }

        while let Ok(mut stream) = stream_muxer.accept_stream().await {
            task::spawn(async move {
                info!("accepted new stream: {:?}", stream);
                let mut buf = [0; 4096];

                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(num) => num,
                        Err(e) => {
                            error!("{:?} read failed: {:?}", stream, e);
                            return;
                        }
                    };
                    if n == 0 {
                        return;
                    }
                    if let Err(e) = stream.write_all(buf[..n].as_ref()).await {
                        error!("{:?} write failed: {:?}", stream, e);
                        return;
                    };
                }
            });
        }
    }
}

async fn run_client() {
    let addr: Multiaddr = "/dns4/localhost/tcp/38087".parse().unwrap();
    let sec = secio::Config::new(Keypair::generate_secp256k1());
    let mux = Selector::new(yamux::Config::client(), mplex::Config::new());

    let mut tu = TransportUpgrade::new(build_pet_config(), mux, sec);

    let mut stream_muxer = tu.dial(addr).await.expect("listener is started already");
    info!("open a new connection: {:?}", stream_muxer);

    if let Some(task) = stream_muxer.task() {
        task::spawn(task);
    }

    let mut stream = stream_muxer.open_stream().await.unwrap();
    task::spawn(async move {
        info!("opened new stream {:?}", stream);
        let data = b"hello world";

        stream.write_all(data.as_ref()).await.unwrap();
        info!("stream: {:?}: write {:?}", stream, String::from_utf8_lossy(data));

        let mut frame = vec![0; data.len()];
        stream.read_exact(&mut frame).await.unwrap();
        info!("stream: {:?}: read {:?}", stream, String::from_utf8_lossy(&frame));

        assert_eq!(&data[..], &frame[..]);
        stream.close().await.expect("close stream");
    })
    .await;

    stream_muxer.close().await.expect("close connection");

    info!("shutdown is completed");
}
