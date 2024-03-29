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

use async_trait::async_trait;
use std::time::Duration;
#[macro_use]
extern crate lazy_static;

use futures::{AsyncReadExt, AsyncWriteExt};
use libp2prs_core::identity::Keypair;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId, ProtocolId};
use libp2prs_runtime::task;
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler, ProtocolImpl};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::{DummyProtocol, Swarm, SwarmError};
use libp2prs_tcp::TcpConfig;
use libp2prs_yamux as yamux;
use std::{collections::VecDeque, error::Error};

fn main() {
    task::block_on(async {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        if std::env::args().nth(1) == Some("server".to_string()) {
            log::info!("Starting server ......");
            run_server();
        } else {
            log::info!("Starting client ......");
            run_client();
        }
    });
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
}

const PROTO_NAME: &[u8] = b"/my/1.0.0";

#[allow(clippy::empty_loop)]
fn run_server() {
    let keys = SERVER_KEY.clone();

    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::server();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    struct MyProtocol;
    #[derive(Clone)]
    struct MyProtocolHandler;

    impl ProtocolImpl for MyProtocol {
        fn handlers(&self) -> Vec<IProtocolHandler> {
            vec![Box::new(MyProtocolHandler)]
        }
    }

    impl UpgradeInfo for MyProtocolHandler {
        type Info = ProtocolId;

        fn protocol_info(&self) -> Vec<Self::Info> {
            vec![ProtocolId::new(PROTO_NAME, 1011)]
        }
    }

    impl Notifiee for MyProtocolHandler {}

    #[async_trait]
    impl ProtocolHandler for MyProtocolHandler {
        async fn handle(&mut self, stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
            log::info!("MyProtocolHandler handling inbound {:?}", stream);
            let (r, mut w) = stream.split();
            if let Err(e) = futures::io::copy(r, &mut w).await {
                if e.kind() != std::io::ErrorKind::UnexpectedEof {
                    return Err(Box::new(SwarmError::from(e)));
                }
            }
            Ok(())
        }

        fn box_clone(&self) -> IProtocolHandler {
            Box::new(self.clone())
        }
    }

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(DummyProtocol::new())
        .with_protocol(MyProtocol)
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    let _control = swarm.control();

    swarm.listen_on(vec![listen_addr]).unwrap();

    swarm.start();

    loop {}
}

fn run_client() {
    let keys = Keypair::generate_secp256k1();

    let _addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::client();
    //let mux = mplex::Config::new();
    //let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_ping(PingConfig::new().with_unsolicited(false).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.start();

    task::block_on(async move {
        control
            .connect_with_addrs(remote_peer_id, vec!["/ip4/127.0.0.1/tcp/8086".parse().unwrap()])
            .await
            .unwrap();
        let mut handles = VecDeque::default();
        for _ in 0..100u32 {
            let mut stream = control
                .new_stream(remote_peer_id, vec![ProtocolId::new(PROTO_NAME, 1011)])
                .await
                .unwrap();
            log::info!("stream {:?} opened, writing something...", stream);

            let handle = task::spawn(async move {
                let msg = b"hello";

                for _ in 0..100u32 {
                    stream.write_all(msg).await.expect("C write");

                    let mut buf = vec![0; msg.len()];
                    stream.read_exact(&mut buf).await.expect("C read");
                    assert_eq!(buf, msg);

                    // runtime::sleep(Duration::from_secs(1)).await;
                }
                stream.close().await.expect("close stream");
            });
            handles.push_back(handle);
        }

        for handle in handles {
            handle.await;
        }

        log::info!("shutdown is completed");
    });
}
