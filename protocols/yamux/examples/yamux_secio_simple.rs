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

use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use log::info;

use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux::{Config, Connection, Mode};

use libp2prs_core::identity::Keypair;
use libp2prs_secio::Config as SecioConfig;

fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Trace).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

fn run_server() {
    let key = Keypair::generate_secp256k1();
    let config = SecioConfig::new(key);

    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (handle, _, _) = config.handshake(socket).await.expect("handshake");
                let conn = Connection::new(handle, Config::default(), Mode::Server);
                let mut ctrl = conn.control();

                task::spawn(async {
                    let mut muxer_conn = conn;
                    while muxer_conn.next_stream().await.is_ok() {}
                    info!("connection is closed");
                });

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    task::spawn(async move {
                        info!("S: accepted new stream");
                        let mut buf = [0; 4096];
                        loop {
                            let n = stream.read2(&mut buf).await.expect("read stream");
                            if n == 0 {
                                info!("stream({}) closed", stream.id());
                                break;
                            }
                            stream.write_all2(buf[..n].as_ref()).await.expect("write stream");
                        }
                    });
                }
            });
        }
    });
}

fn run_client() {
    let key = Keypair::generate_secp256k1();
    let config = SecioConfig::new(key);

    task::block_on(async move {
        let socket = TcpStream::connect("127.0.0.1:12345").await.expect("connect");
        info!("[client] connected to server: {:?}", socket.peer_addr());
        let (handle, _, _) = config.handshake(socket).await.expect("handshake");

        let conn = Connection::new(handle, Config::default(), Mode::Client);
        let mut ctrl = conn.control();

        task::spawn(async {
            let mut muxer_conn = conn;
            while muxer_conn.next_stream().await.is_ok() {}
            info!("connection is closed");
        });

        let mut stream = ctrl.open_stream().await.unwrap();
        info!("C: opened new stream {}", stream.id());

        let data = b"hello world";

        stream.write_all2(data).await.unwrap();
        info!("C: {}: wrote {} bytes", stream.id(), data.len());

        let mut frame = vec![0; data.len()];
        stream.read_exact2(&mut frame).await.unwrap();

        info!("C: {}: read {} bytes", stream.id(), frame.len());

        assert_eq!(&data[..], &frame[..]);

        ctrl.close().await.expect("close connection");

        //Ok::<(), yamux::ConnectionError>(())
    });
}
