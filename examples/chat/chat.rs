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
use async_trait::async_trait;
use std::time::Duration;
#[macro_use]
extern crate lazy_static;

use async_std::io;
use std::io::Write;

use libp2prs_core::identity::Keypair;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_secio as secio;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, ProtocolHandler};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::{Swarm, SwarmError};
use libp2prs_tcp::TcpConfig;
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux as yamux;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        log::info!("Starting server ......");
        run_server();
    } else {
        log::info!("Starting client ......");
        run_client();
    }
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
}

async fn write_data<C>(mut stream: C)
where
    C: ReadEx + WriteEx,
{
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        let n = io::stdin().read_line(&mut input).await.unwrap();
        let _ = stream.write_all2(&input.as_bytes()[0..n]).await;
        let _ = stream.flush2().await;
    }
}

async fn read_data<C>(mut stream: C)
where
    C: ReadEx + WriteEx,
{
    loop {
        let mut buf = [0; 4096];
        let n = stream.read2(&mut buf).await.unwrap();
        let str = String::from_utf8_lossy(&buf[0..n]);
        if str == "" {
            return;
        }
        if str != "\n" {
            print!("\x1b[32m{}\x1b[0m> ", str);
            let _ = std::io::stdout().flush();
        }
    }
}

#[derive(Clone)]
struct ChatHandler;

impl UpgradeInfo for ChatHandler {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/chat/1.0.0"]
    }
}

#[async_trait]
impl ProtocolHandler for ChatHandler {
    async fn handle(&mut self, stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        let stream = stream;
        log::trace!("ChatHandler handling inbound {:?}", stream);
        let s1 = stream.clone();
        let s2 = stream.clone();

        task::spawn(async {
            read_data(s1).await;
        });

        task::spawn(async {
            write_data(s2).await;
        });
        Ok(())
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

fn run_server() {
    let keys = SERVER_KEY.clone();
    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8086".parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(PeerId::from_public_key(keys.public()))
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(ChatHandler {}));

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
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(PeerId::from_public_key(keys.public())).with_transport(Box::new(tu));

    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    //let remote_peer_id = PeerId::from_str("QmQEBrDwz5MU8gpnrJz77vmAebRUtrqfoxsQxPsWSBeQUU").unwrap();

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm
        .peers
        .addrs
        .add_addr(&remote_peer_id, "/ip4/127.0.0.1/tcp/8086".parse().unwrap(), Duration::default());

    swarm.start();

    task::block_on(async move {
        control.new_connection(remote_peer_id.clone()).await.unwrap();
        let stream = control.new_stream(remote_peer_id, vec![b"/chat/1.0.0"]).await.unwrap();

        log::info!("stream {:?} opened, writing something...", stream);
        let s1 = stream.clone();
        let s2 = stream.clone();
        task::spawn(async {
            read_data(s1).await;
        });

        task::spawn(async {
            write_data(s2).await;
        });

        loop {
            task::sleep(Duration::from_secs(10)).await;
        }
    });
}