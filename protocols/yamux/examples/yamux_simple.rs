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

use libp2prs_traits::{copy, ReadEx, WriteEx};
use libp2prs_yamux::{connection::Connection, connection::Mode, Config};

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

fn run_server() {
    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            info!("accepted a socket: {:?}", socket.peer_addr());

            task::spawn(async move {
                let conn = Connection::new(socket, Config::default(), Mode::Server);
                let mut ctrl = conn.control();

                task::spawn(async {
                    let mut muxer_conn = conn;
                    while muxer_conn.next_stream().await.is_ok() {}
                    info!("connection is closed");
                });

                while let Ok(stream) = ctrl.accept_stream().await {
                    info!("accepted new stream: {}", stream.id());
                    task::spawn(async move {
                        let r = stream.clone();
                        let w = stream.clone();
                        let _ = copy(r, w).await;
                    });
                }
            });
        }
    });
}

fn run_client() {
    task::block_on(async move {
        let socket = TcpStream::connect("127.0.0.1:12345").await.unwrap();
        info!("[client] connected to server: {:?}", socket.peer_addr());

        let conn = Connection::new(socket, Config::default(), Mode::Client);
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

        let _ = stream.close2().await;

        ctrl.close().await.expect("close connection");

        //Ok::<(), yamux::ConnectionError>(())
    });
}
