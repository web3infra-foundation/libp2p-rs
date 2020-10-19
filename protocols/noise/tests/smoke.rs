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
use libp2p_core::identity;
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_noise::{Keypair, NoiseConfig, X25519};
use libp2p_traits::{ReadEx, WriteEx};
use log::info;

//
// use futures::{
//     future::{self, Either},
//     prelude::*,
// };
// use libp2p_core::identity;
// use libp2p_core::transport::{ListenerEvent, Transport};
// use libp2p_core::upgrade::{self, apply_inbound, apply_outbound, Negotiated};
// use libp2p_noise::{
//     Keypair, NoiseConfig, NoiseError, NoiseOutput, RemoteIdentity, X25519Spec, X25519,
// };
// use libp2p_tcp::{TcpConfig, TcpTransStream};
// use log::info;
// use quickcheck::QuickCheck;
// use std::{convert::TryInto, io};
//
// #[allow(dead_code)]
// fn core_upgrade_compat() {
//     // Tests API compaibility with the libp2p-core upgrade API,
//     // i.e. if it compiles, the "test" is considered a success.
//     let id_keys = identity::Keypair::generate_ed25519();
//     let dh_keys = Keypair::<X25519>::new().into_authentic(&id_keys).unwrap();
//     let noise = NoiseConfig::xx(dh_keys, id_keys).into_authenticated();
//     let _ = TcpConfig::new()
//         .upgrade(upgrade::Version::V1)
//         .authenticate(noise);
// }
//
#[test]
fn xx() {
    task::block_on(async {

        // server
        task::spawn(async {
            let server_id = identity::Keypair::generate_ed25519();
            let server_auth = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
            let server_config = NoiseConfig::xx(server_auth, server_id);

            let listener = async_std::net::TcpListener::bind("127.0.0.1:9876").await.unwrap();
            while let Ok((socket, _)) = listener.accept().await {
                let cfg = server_config.clone();
                task::spawn(async move {
                    let (_, mut output) = cfg.handshake(socket, false).await.unwrap();

                    info!("handshake finished");

                    let mut buf = [0; 100];

                    loop {
                        info!("outside loop");
                        if let Ok(n) = output.read2(&mut buf).await {
                            // info!("public key is {:?}", b.remote_pub_key());
                            info!("data is {:?}", buf.to_vec());
                            // let mut buffer = Vec::from(buf[..11]);
                            let u = b"!";
                            // buffer.push(u[0]);
                            buf[11] = u[0];
                            if output.write_all2(&buf).await.is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                });
            }
        });

        let client_id = identity::Keypair::generate_ed25519();
        let client_auth = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let client_config = NoiseConfig::xx(client_auth, client_id);

        //client
        let client_socket = async_std::net::TcpStream::connect("127.0.0.1:9876").await.unwrap();
        let (_, mut output) = client_config
            .handshake(client_socket, true)
            .await
            .unwrap();
        let data = b"hello world";
        output.write2(data).await;
        info!("write finished");
        let mut buf = vec![0u8; 100];
        output.read2(buf.as_mut()).await;
        info!("read finished, {:?}", String::from_utf8(buf).unwrap());
    });
}
