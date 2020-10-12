use async_std::task;
use libp2p_core::identity;
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_noise::{Keypair, X25519};
use libp2p_traits::{ReadEx, WriteEx};
use log::info;
use noise::{Keypair, NoiseConfig, X25519};

// // Copyright 2019 Parity Technologies (UK) Ltd.
// //
// // Permission is hereby granted, free of charge, to any person obtaining a
// // copy of this software and associated documentation files (the "Software"),
// // to deal in the Software without restriction, including without limitation
// // the rights to use, copy, modify, merge, publish, distribute, sublicense,
// // and/or sell copies of the Software, and to permit persons to whom the
// // Software is furnished to do so, subject to the following conditions:
// //
// // The above copyright notice and this permission notice shall be included in
// // all copies or substantial portions of the Software.
// //
// // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// // OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// // AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// // FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// // DEALINGS IN THE SOFTWARE.
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
        let server_id = identity::Keypair::generate_ed25519();
        let client_id = identity::Keypair::generate_ed25519();

        let server_auth = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
        let client_auth = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();

        let server_config = NoiseConfig::xx(server_auth, server_id);
        let client_config = NoiseConfig::xx(client_auth, client_id);

        // server
        task::spawn(async {
            let listener = async_std::net::TcpListener::bind("127.0.0.1:9876").await.unwrap();
            while let Ok((socket, _)) = listener.accept().await {
                task::spawn(async move {
                    let (_, output) = server_config.handshake(listener, false).await.unwrap();

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

        //client
        let socket = async_std::net::TcpStream::connect("127.0.0.1:9876").await.unwrap();
        let output = client_config
            .upgrade_outbound(socket, b"/noise/xx/25519/chachapoly/sha256/0.1.0")
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
//
// type Output<C> = (RemoteIdentity<C>, NoiseOutput<Negotiated<TcpTransStream>>);
//
// fn run<T, U, I, C>(server_transport: T, client_transport: U, messages: I)
//     where
//         T: Transport<Output=Output<C>>,
//         T::Dial: Send + 'static,
//         T::Listener: Send + Unpin + 'static,
//         T::ListenerUpgrade: Send + 'static,
//         U: Transport<Output=Output<C>>,
//         U::Dial: Send + 'static,
//         U::Listener: Send + 'static,
//         U::ListenerUpgrade: Send + 'static,
//         I: IntoIterator<Item=Message> + Clone,
// {
//     futures::executor::block_on(async {
//         let mut server: T::Listener = server_transport
//             .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
//             .unwrap();
//
//         let server_address = server
//             .try_next()
//             .await
//             .expect("some event")
//             .expect("no error")
//             .into_new_address()
//             .expect("listen address");
//
//         let outbound_msgs = messages.clone();
//         let client_fut = async {
//             let mut client_session = client_transport
//                 .dial(server_address.clone())
//                 .unwrap()
//                 .await
//                 .map(|(_, session)| session)
//                 .expect("no error");
//
//             for m in outbound_msgs {
//                 let n = (m.0.len() as u64).to_be_bytes();
//                 client_session.write_all(&n[..]).await.expect("len written");
//                 client_session.write_all(&m.0).await.expect("no error")
//             }
//             client_session.flush().await.expect("no error");
//         };
//
//         let server_fut = async {
//             let mut server_session = server
//                 .try_next()
//                 .await
//                 .expect("some event")
//                 .map(ListenerEvent::into_upgrade)
//                 .expect("no error")
//                 .map(|client| client.0)
//                 .expect("listener upgrade")
//                 .await
//                 .map(|(_, session)| session)
//                 .expect("no error");
//
//             for m in messages {
//                 let len = {
//                     let mut n = [0; 8];
//                     match server_session.read_exact(&mut n).await {
//                         Ok(()) => u64::from_be_bytes(n),
//                         Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => 0,
//                         Err(e) => panic!("error reading len: {}", e),
//                     }
//                 };
//                 info!("server: reading message ({} bytes)", len);
//                 let mut server_buffer = vec![0; len.try_into().unwrap()];
//                 server_session
//                     .read_exact(&mut server_buffer)
//                     .await
//                     .expect("no error");
//                 assert_eq!(server_buffer, m.0)
//             }
//         };
//
//         futures::future::join(server_fut, client_fut).await;
//     })
// }
//
// fn expect_identity<C>(
//     output: Output<C>,
//     pk: &identity::PublicKey,
// ) -> impl Future<Output=Result<Output<C>, NoiseError>> {
//     match output.0 {
//         RemoteIdentity::IdentityKey(ref k) if k == pk => future::ok(output),
//         _ => panic!("Unexpected remote identity"),
//     }
// }
//
// #[derive(Debug, Clone, PartialEq, Eq)]
// struct Message(Vec<u8>);
//
// impl quickcheck::Arbitrary for Message {
//     fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
//         let s = 1 + g.next_u32() % (128 * 1024);
//         let mut v = vec![0; s.try_into().unwrap()];
//         g.fill_bytes(&mut v);
//         Message(v)
//     }
// }