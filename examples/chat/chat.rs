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
use libp2prs_runtime::task;
use std::time::Duration;

#[macro_use]
extern crate lazy_static;

use std::{error::Error, io::Write};

use libp2prs_core::identity::Keypair;
use libp2prs_core::multiaddr::protocol::Protocol;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId, ProtocolId};
use libp2prs_secio as secio;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, Notifiee, ProtocolHandler, ProtocolImpl};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::Swarm;
use libp2prs_tcp::TcpConfig;
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux as yamux;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Config {
    client_or_server: String,

    /// Destination multiaddr string
    #[structopt(short = "d")]
    dest_multiaddr: Option<String>,

    /// The port to connect to
    #[structopt(short = "s")]
    source_port: Option<u16>,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = Config::from_args();
    if options.client_or_server == "server" && options.source_port.is_some() {
        log::info!("Starting server ......");
        run_server();
    } else if options.client_or_server == "client" && options.dest_multiaddr.is_some() {
        log::info!("Starting client ......");
        run_client();
    } else {
        panic!("param error")
    }
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
}
const PROTO_NAME: &[u8] = b"/chat/1.0.0";

async fn write_data<C>(mut stream: C)
where
    C: ReadEx + WriteEx,
{
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input).unwrap();
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

struct Chat;

impl ProtocolImpl for Chat {
    fn handler(&self) -> IProtocolHandler {
        Box::new(ChatHandler)
    }
}

#[derive(Clone)]
struct ChatHandler;

impl UpgradeInfo for ChatHandler {
    type Info = ProtocolId;

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![PROTO_NAME.into()]
    }
}

impl Notifiee for ChatHandler {}

#[async_trait]
impl ProtocolHandler for ChatHandler {
    async fn handle(&mut self, stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
        let stream = stream;
        log::trace!("ChatHandler handling inbound {:?}", stream);
        let s1 = stream.clone();

        task::spawn(async {
            read_data(stream).await;
        });

        task::spawn(async {
            write_data(s1).await;
        });
        Ok(())
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

#[allow(clippy::empty_loop)]
fn run_server() {
    let keys = SERVER_KEY.clone();
    let options = Config::from_args();
    let listen_addr = format!("/ip4/127.0.0.1/tcp/{}", &(options.source_port.unwrap())).parse().unwrap();
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let mut swarm = Swarm::new(keys.public()).with_transport(Box::new(tu)).with_protocol(Chat);

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    let _control = swarm.control();

    swarm.listen_on(vec![listen_addr]).unwrap();

    swarm.start();
    loop {}
}

fn run_client() {
    let keys = Keypair::generate_secp256k1();
    let options = Config::from_args();
    let mut dial_addr = Multiaddr::from_str(&(options.dest_multiaddr.unwrap())).unwrap();
    let last_protocol = dial_addr.pop().unwrap();
    let remote_peer_id = match last_protocol {
        Protocol::P2p(data) => PeerId::from_multihash(data).unwrap(),
        _ => panic!("expect p2p protocol"),
    };
    let sec = secio::Config::new(keys.clone());
    let mux = yamux::Config::new();
    let tu = TransportUpgrade::new(TcpConfig::default(), mux, sec);

    let swarm = Swarm::new(keys.public()).with_transport(Box::new(tu));

    let mut control = swarm.control();

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.start();

    task::block_on(async move {
        control.connect_with_addrs(remote_peer_id, vec![dial_addr]).await.unwrap();
        let stream = control.new_stream(remote_peer_id, vec![PROTO_NAME.into()]).await.unwrap();

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
