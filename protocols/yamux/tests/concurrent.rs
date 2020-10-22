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
use futures::{channel::mpsc, prelude::*};
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux::{connection::Connection, connection::Mode, error::ConnectionError, Config};
use std::collections::VecDeque;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};

#[allow(dead_code)]
async fn roundtrip(address: SocketAddr, nstreams: usize, data: Arc<Vec<u8>>) {
    let listener = TcpListener::bind(&address).await.expect("bind");
    let address = listener.local_addr().expect("local address");

    let server = async move {
        let socket = listener.accept().await.expect("accept").0;
        let conn = Connection::new(socket, Config::default(), Mode::Server);
        let mut ctrl = conn.control();

        let mut handles = VecDeque::new();
        let loop_handle = task::spawn(async {
            let mut muxer_conn = conn;
            while muxer_conn.next_stream().await.is_ok() {}
            log::info!("S connection is closed");
        });

        while let Ok(mut stream) = ctrl.accept_stream().await {
            log::debug!("S: accepted new stream");
            let handle = task::spawn(async move {
                let mut len = [0; 4];
                stream.read_exact2(&mut len).await?;
                let mut buf = vec![0; u32::from_be_bytes(len) as usize];
                let _ = stream.read_exact2(&mut buf).await;
                stream.write_all2(&buf).await?;
                stream.close2().await?;
                Ok::<(), ConnectionError>(())
            });
            handles.push_back(handle);
        }

        while let Some(handle) = handles.pop_front() {
            handle.await.expect("stream task");
        }
        loop_handle.await;
    };

    let server_handle = task::spawn(server);

    let socket = TcpStream::connect(&address).await.expect("connect");
    let (tx, rx) = mpsc::unbounded();
    let conn = Connection::new(socket, Config::default(), Mode::Client);
    let mut ctrl = conn.control();

    let mut handles = VecDeque::new();
    let loop_handle = task::spawn(async {
        let mut muxer_conn = conn;
        while muxer_conn.next_stream().await.is_ok() {}
        log::info!("C connection is closed");
    });

    for _ in 0..nstreams {
        let data = data.clone();
        let tx = tx.clone();
        let mut ctrl = ctrl.clone();
        let handle = task::spawn(async move {
            let mut stream = ctrl.open_stream().await?;
            log::debug!("C: opened new stream {}", stream.id());
            stream.write_all2(&(data.len() as u32).to_be_bytes()[..]).await?;
            stream.write_all2(&data).await?;
            stream.close2().await?;
            log::debug!("C: {}: wrote {} bytes", stream.id(), data.len());
            let mut frame = vec![0; data.len()];
            stream.read_exact2(&mut frame).await?;
            log::debug!("C: {}: read {} bytes", stream.id(), frame.len());
            assert_eq!(&data[..], &frame[..]);
            tx.unbounded_send(1).expect("unbounded_send");
            Ok::<(), ConnectionError>(())
        });
        handles.push_back(handle);
    }
    let n = rx.take(nstreams).fold(0, |acc, n| future::ready(acc + n)).await;
    ctrl.close().await.expect("close connection");
    assert_eq!(nstreams, n);

    while let Some(handle) = handles.pop_front() {
        let _ = handle.await;
    }
    loop_handle.await;

    server_handle.await;
}

#[test]
fn concurrent_streams() {
    // env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    let data = Arc::new(vec![0x42; 100 * 1024]);
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
    task::block_on(roundtrip(addr, 1000, data))
}
