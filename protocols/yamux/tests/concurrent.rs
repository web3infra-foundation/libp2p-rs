// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use futures::{channel::mpsc, prelude::*};
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux::{
    connection::Connection,
    connection::Mode,
    Config,
    error::ConnectionError,
};
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
                stream.read_exact2(&mut buf).await;
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
        handle.await;
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
