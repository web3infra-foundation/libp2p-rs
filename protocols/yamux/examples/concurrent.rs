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

use async_std::{net::TcpStream, task};
use futures::{channel::mpsc, prelude::*};
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux as yamux;
use libp2prs_yamux::{Config, Connection, Mode};
use std::sync::Arc;

/*
use log::LevelFilter;
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config as LogConfig, Root},
    encode::pattern::PatternEncoder,
    Handle,
};

fn setup_logger(path: &'static str, level: LevelFilter) -> Handle {
    let log_file = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("[{l}] {M} {m}\n")))
        .build(path).expect("build log file");

    let cfg = LogConfig::builder()
        .appender(Appender::builder().build("logfile", Box::new(log_file)))
        .build(
            Root::builder().appender("logfile").build(level)
        ).expect("build log config");

    let handle = log4rs::init_config(cfg).expect("init log");
    handle
}
 */

fn main() {
    // let _handle = setup_logger("/Users/dian/tmp/rust/netwarps/client.log", LevelFilter::Info);
    // env_logger::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let data = Arc::new(vec![0x42; 100 * 1024]);

    let nstreams = 1000_usize;
    task::block_on(async move {
        let socket = TcpStream::connect("127.0.0.1:8888").await.expect("connect");
        let (tx, rx) = mpsc::unbounded();
        let conn = Connection::new(socket, Config::default(), Mode::Client);
        let mut ctrl = conn.control();
        task::spawn(async {
            let mut muxer_conn = conn;
            while muxer_conn.next_stream().await.is_ok() {}
            log::info!("connection is closed");
        });

        for _ in 0..nstreams {
            let data = data.clone();
            let tx = tx.clone();
            let mut ctrl = ctrl.clone();
            task::spawn(async move {
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
                Ok::<(), yamux::ConnectionError>(())
            });
        }
        let n = rx.take(nstreams).fold(0, |acc, n| future::ready(acc + n)).await;
        ctrl.close().await.expect("close connection");
        assert_eq!(nstreams, n)
    })
}
