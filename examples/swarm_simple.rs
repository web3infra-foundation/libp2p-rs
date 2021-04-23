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
use futures::{AsyncReadExt, AsyncWriteExt};
use std::{error::Error, time::Duration};
#[macro_use]
extern crate lazy_static;

use libp2prs_core::identity::Keypair;
use libp2prs_core::peerstore::ADDRESS_TTL;
use libp2prs_core::transport::memory::MemoryTransport;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::{Selector, UpgradeInfo};
use libp2prs_core::{Multiaddr, PeerId, ProtocolId};
use libp2prs_exporter::ExporterServer;
use libp2prs_infoserver::InfoServer;
use libp2prs_mplex as mplex;
use libp2prs_runtime::task;
use libp2prs_secio as secio;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::ping::PingConfig;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler, ProtocolImpl};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::{DummyProtocol, Swarm};
use libp2prs_tcp::TcpConfig;
use libp2prs_websocket::WsConfig;
use libp2prs_yamux as yamux;

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

    let listen_addr1: Multiaddr = "/ip4/0.0.0.0/tcp/8086".parse().unwrap();
    let listen_addr2: Multiaddr = "/ip4/127.0.0.1/tcp/30199/ws".parse().unwrap();

    let sec = secio::Config::new(keys.clone());
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux.clone(), sec.clone());
    let tu2 = TransportUpgrade::new(MemoryTransport::default(), mux.clone(), sec.clone());
    let tu3 = TransportUpgrade::new(WsConfig::new(), mux, sec);

    struct MyProtocol;
    impl ProtocolImpl for MyProtocol {
        fn handler(&self) -> IProtocolHandler {
            Box::new(MyProtocolHandler)
        }
    }

    #[derive(Clone)]
    struct MyProtocolHandler;

    impl UpgradeInfo for MyProtocolHandler {
        type Info = ProtocolId;

        fn protocol_info(&self) -> Vec<Self::Info> {
            vec![PROTO_NAME.into()]
        }
    }

    impl Notifiee for MyProtocolHandler {}

    #[async_trait]
    impl ProtocolHandler for MyProtocolHandler {
        async fn handle(&mut self, stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
            let mut stream = stream;
            log::trace!("MyProtocol handling inbound {:?}", stream);
            let mut msg = vec![0; 4096];
            loop {
                let n = stream.read(&mut msg).await?;
                log::info!("received: {:?}", &msg[..n]);
                stream.write(&msg[..n]).await?;
            }
        }

        fn box_clone(&self) -> IProtocolHandler {
            Box::new(self.clone())
        }
    }

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_transport(Box::new(tu2))
        .with_transport(Box::new(tu3))
        .with_protocol(DummyProtocol::new())
        .with_protocol(MyProtocol)
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    let control = swarm.control();

    swarm.listen_on(vec![listen_addr1, listen_addr2]).unwrap();

    let monitor = InfoServer::new(control.clone());

    monitor.start("127.0.0.1:8999".to_string());

    ExporterServer::new(control).start("127.0.0.1:9102".to_string());

    swarm.start();

    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

fn run_client() {
    let keys = Keypair::generate_secp256k1();

    let sec = secio::Config::new(keys.clone());
    let mux = Selector::new(yamux::Config::new(), mplex::Config::new());
    let tu = TransportUpgrade::new(TcpConfig::default(), mux.clone(), sec.clone());
    let tu2 = TransportUpgrade::new(WsConfig::new(), mux, sec);

    let swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_transport(Box::new(tu2))
        .with_ping(PingConfig::new().with_unsolicited(false).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));

    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.start();

    // add a peer to peer store manually
    let addrs = vec![
        "/ip4/127.0.0.1/tcp/30199/ws".parse().unwrap(),
        "/ip4/127.0.0.1/tcp/8086".parse().unwrap(),
    ];
    control.add_addrs(&remote_peer_id, addrs, ADDRESS_TTL);

    task::block_on(async move {
        for _ in 0..2 {
            // method 1
            //let mut connection = control.new_connection(remote_peer_id.clone()).await.unwrap();
            //let mut stream = connection.open_stream(vec![b"/my/1.0.0"], |r| r.unwrap()).await;

            // method 2
            let mut stream = control.new_stream(remote_peer_id, vec![PROTO_NAME.into()]).await.unwrap();
            log::info!("stream {:?} opened, writing something...", stream);
            let _ = stream.write_all(b"hello").await;

            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let str = String::from_utf8_lossy(&buf[0..n]);

            log::info!("======================{}", str);

            task::sleep(Duration::from_secs(1)).await;

            let _ = stream.close().await;

            log::info!("shutdown is completed");
        }

        // close the swarm explicitly
        control.close();
    });
}
