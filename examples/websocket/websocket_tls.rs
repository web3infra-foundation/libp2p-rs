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

use libp2prs_core::identity::Keypair;
use libp2prs_core::transport::upgrade::TransportUpgrade;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_mplex as mplex;
use libp2prs_plaintext as plaintext;
use libp2prs_swarm::identify::IdentifyConfig;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, ProtocolHandler};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::{Swarm, SwarmError};
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_websocket::{tls, WsConfig};

use async_std::io;
use rustls::internal::pemfile::{certs, rsa_private_keys};
use rustls::{Certificate, PrivateKey};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

/// run server:
/// RUST_LOG=trace cargo run --example websocket_tls server
/// run client:
/// RUST_LOG=trace cargo run --example websocket_tls client
#[allow(dead_code)]
#[derive(StructOpt)]
struct ServerTlsConfig {
    client_or_server: String,

    /// The host to connect to
    #[structopt(short = "h", long = "host", default_value = "127.0.0.1")]
    host: String,

    /// The port to connect to
    #[structopt(short = "p", long = "port", default_value = "31443")]
    port: u16,

    /// cert file
    #[structopt(short = "c", long = "cert", parse(from_os_str), default_value = "examples/websocket/cert/end.cert")]
    cert: PathBuf,

    /// key file
    #[structopt(short = "k", long = "key", parse(from_os_str), default_value = "examples/websocket/cert/end.rsa")]
    key: PathBuf,
}

#[allow(dead_code)]
#[derive(StructOpt)]
struct ClientTlsConfig {
    client_or_server: String,

    /// The host to connect to
    #[structopt(short = "h", long = "host", default_value = "127.0.0.1")]
    host: String,

    /// The port to connect to
    #[structopt(short = "p", long = "port", default_value = "31443")]
    port: u16,

    /// The domain to connect to. This may be different from the host!
    #[structopt(short = "d", long = "domain", default_value = "localhost")]
    domain: String,

    /// A file with a certificate authority chain, allows to connect
    /// to certificate authories not included in the default set
    #[structopt(
        short = "c",
        long = "cafile",
        parse(from_os_str),
        default_value = "examples/websocket/cert/ca.cert"
    )]
    cafile: PathBuf,
}

/// Load the passed certificates file
fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?)).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
}

/// Load the passed keys file
fn load_keys(path: &Path) -> io::Result<Vec<PrivateKey>> {
    rsa_private_keys(&mut BufReader::new(File::open(path)?)).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
}

/// A TLS server needs a certificate and a fitting private key
fn load_config(options: &ServerTlsConfig) -> io::Result<(PrivateKey, Vec<Certificate>)> {
    let certs = load_certs(&options.cert)?;
    let keys = load_keys(&options.key)?;
    let one = keys.get(0).unwrap().to_owned();
    Ok((one, certs))
}

// Build tls client config
fn build_client_tls_config(options: &ClientTlsConfig) -> tls::Config {
    let ca = load_certs(&options.cafile).unwrap();
    let ca_cert = &tls::Certificate::new(ca.get(0).unwrap().as_ref().to_vec());
    log::trace!("ca cert  {:?}", &ca_cert);
    let mut builder = tls::Config::builder();
    builder.add_trust(ca_cert).unwrap().clone().finish()
}
// Build tls server config
fn build_server_tls_config(options: &ServerTlsConfig) -> tls::Config {
    let (pk, certs) = load_config(&options).unwrap();
    log::trace!("pk  {:?}", &pk);
    log::trace!("certs  {:?}", &certs);
    let cert_iter = certs.into_iter().map(|c| tls::Certificate::new(c.0));
    tls::Config::new(tls::PrivateKey::new(pk.0), cert_iter).unwrap()
}

#[derive(Clone)]
struct MyProtocolHandler;

impl UpgradeInfo for MyProtocolHandler {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/my/1.0.0"]
    }
}

#[async_trait]
impl ProtocolHandler for MyProtocolHandler {
    async fn handle(&mut self, stream: Substream, _info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        let mut stream = stream;
        log::trace!("MyProtocolHandler handling inbound {:?}", stream);
        let mut msg = vec![0; 4096];
        loop {
            let n = stream.read2(&mut msg).await?;
            log::info!("received: {:?}", &msg[..n]);
            stream.write2(&msg[..n]).await?;
        }
    }

    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

fn main() -> io::Result<()> {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        log::info!("Starting server ......");
        run_server()
    } else {
        log::info!("Starting client ......");
        run_client()
    }
}

lazy_static! {
    static ref SERVER_KEY: Keypair = Keypair::generate_ed25519_fixed();
}

#[allow(clippy::empty_loop)]
fn run_server() -> io::Result<()> {
    let options = ServerTlsConfig::from_args();
    let addr = format!("/ip4/{}/tcp/{}/wss", &options.host, &options.port);
    log::info!("server addr1 {}", &addr);
    let keys = SERVER_KEY.clone();

    let listen_addr: Multiaddr = addr.parse().unwrap();

    let sec = plaintext::PlainTextConfig::new(keys.clone());
    let mux = mplex::Config::new();

    let ws = WsConfig::new().set_tls_config(build_server_tls_config(&options)).to_owned();
    let tu = TransportUpgrade::new(ws, mux, sec);

    let mut swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(MyProtocolHandler))
        .with_identify(IdentifyConfig::new(false));

    log::info!("Swarm created, local-peer-id={:?}", swarm.local_peer_id());

    let _control = swarm.control();

    swarm.listen_on(vec![listen_addr]).unwrap();

    swarm.start();

    loop {}
}

fn run_client() -> io::Result<()> {
    let options = ClientTlsConfig::from_args();
    let addr = format!("/dns4/{}/tcp/{}/wss", &options.domain, &options.port);
    log::info!("client addr {}", &addr);

    let keys = Keypair::generate_secp256k1();
    let addr: Multiaddr = addr.parse().unwrap();

    let sec = plaintext::PlainTextConfig::new(keys.clone());
    let mux = mplex::Config::new();

    let ws = WsConfig::new_with_dns()
        .set_tls_config(build_client_tls_config(&options))
        .to_owned();
    let tu = TransportUpgrade::new(ws, mux, sec);

    let mut swarm = Swarm::new(keys.public()).with_transport(Box::new(tu));

    let mut control = swarm.control();

    let remote_peer_id = PeerId::from_public_key(SERVER_KEY.public());

    log::info!("about to connect to {:?}", remote_peer_id);

    swarm.peer_addrs_add(&remote_peer_id, addr, Duration::default());

    swarm.start();

    task::block_on(async move {
        control.new_connection(remote_peer_id.clone()).await.unwrap();
        let mut stream = control.new_stream(remote_peer_id, vec![b"/my/1.0.0"]).await.unwrap();
        log::info!("stream {:?} opened, writing something...", stream);

        let msg = b"hello";
        let _ = stream.write2(msg).await;
        let mut buf = [0; 5];

        let _ = stream.read2(&mut buf).await;
        log::info!("receiv msg ======={}", String::from_utf8_lossy(&buf[..]));
        assert_eq!(msg, &buf);

        let _ = stream.close2().await;

        log::info!("shutdown is completed");
    });
    Ok(())
}
