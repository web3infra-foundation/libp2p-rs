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

use async_std::io;
use futures::AsyncWriteExt;
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux::{connection::Connection, connection::Mode, Config};

async fn write_data<C>(mut stream: C)
where
    C: ReadEx + WriteEx + Send,
{
    loop {
        print!("> ");
        let _ = io::stdout().flush().await;
        let mut input = String::new();
        let n = io::stdin().read_line(&mut input).await.unwrap();
        let _ = stream.write_all2(&input.as_bytes()[0..n]).await;
        let _ = stream.flush2().await;
    }
}

async fn read_data<C>(mut stream: C)
where
    C: ReadEx + WriteEx + Send,
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
            let _ = io::stdout().flush().await;
        }
    }
}

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

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    info!("accepted new stream: {}", stream.id());

                    let reader = stream.clone();
                    let read_handle = task::spawn(async move {
                        read_data(reader).await;
                    });

                    let writer = stream.clone();
                    let write_handle = task::spawn(async move {
                        write_data(writer).await;
                    });

                    futures::future::join(read_handle, write_handle).await;

                    stream.close2().await;
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

        let reader = stream.clone();
        let read_handle = task::spawn(async move {
            read_data(reader).await;
        });

        let writer = stream.clone();
        let write_handle = task::spawn(async move {
            write_data(writer).await;
        });

        futures::future::join(read_handle, write_handle).await;

        stream.close2().await;

        ctrl.close().await.expect("close connection");

        //Ok::<(), yamux::ConnectionError>(())
    });
}
