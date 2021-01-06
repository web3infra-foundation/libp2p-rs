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
use libp2prs_core::{identity, PeerId};
use libp2prs_noise::{Keypair, NoiseConfig, X25519Spec};
use libp2prs_traits::{ReadEx, WriteEx};
use log::info;
use log::LevelFilter;
use std::string::ToString;

fn main() {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client .......");
        run_client();
    }
}

fn run_server() {
    task::block_on(async {
        let server_id = identity::Keypair::generate_ed25519_fixed();
        // let server_id_public = server_id.public();
        let pid = PeerId::from(server_id.public());
        info!("I am {}", pid);

        let listener = async_std::net::TcpListener::bind("127.0.0.1:3214").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let server_id = server_id.clone();
            task::spawn(async move {
                let server_dh = Keypair::<X25519Spec>::new().into_authentic(&server_id).unwrap();

                let config = NoiseConfig::xx(server_dh, server_id);

                let (_a, mut b) = config.handshake(socket, false).await.unwrap();

                info!("handshake finished");

                let mut buf = [0; 100];

                loop {
                    info!("outside loop");
                    if let Ok(_n) = b.read2(&mut buf).await {
                        // info!("public key is {:?}", b.remote_pub_key());
                        info!("data is {:?}", buf.to_vec());
                        // let mut buffer = Vec::from(buf[..11]);
                        let u = b"!";
                        // buffer.push(u[0]);
                        buf[11] = u[0];
                        if b.write_all2(&buf).await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            });
        }
    })
}

fn run_client() {
    task::block_on(async {
        let socket = async_std::net::TcpStream::connect("127.0.0.1:3214").await.unwrap();
        info!("[client] connected to server: {:?}", socket.peer_addr());

        let client_id = identity::Keypair::generate_ed25519();
        // let client_id_public = client_id.public();

        let client_dh = Keypair::<X25519Spec>::new().into_authentic(&client_id).unwrap();

        let config = NoiseConfig::xx(client_dh, client_id);

        let (_a, mut b) = config.handshake(socket, true).await.unwrap();
        info!("Handshake finished");

        let data = b"hello world";
        let _ = b.write_all2(data).await;
        info!("write finished");
        let mut buf = vec![0u8; 100];
        let nr = b.read2(buf.as_mut()).await.unwrap();
        info!("read finished, {:?}", &buf[..nr]);
    })
}
