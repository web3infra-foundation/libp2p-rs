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

use async_std::{net::TcpListener, task};
use libp2p_traits::{ReadEx, WriteEx};
use std::collections::HashSet;
use yamux::{Config, Connection, Mode};

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
    // env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    // let _handle = setup_logger("/Users/dian/tmp/rust/netwarps/server.log", LevelFilter::Info);

    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:8888").await.expect("bind");
        while let Ok((socket, _addr)) = listener.accept().await {
            task::spawn(async move {
                let conn = Connection::new(socket, Config::default(), Mode::Server);
                let mut ctrl = conn.control();

                task::spawn(async {
                    let mut muxer_conn = conn;
                    while muxer_conn.next_stream().await.is_ok() {}
                    log::info!("connection is closed");
                });

                let mut set = HashSet::new();

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    log::info!("S: accepted new stream {}", stream.id());
                    set.insert(stream.id());
                    task::spawn(async move {
                        let mut len = [0; 4];
                        if let Err(e) = stream.read_exact2(&mut len).await {
                            log::error!("{} read failed: {:?}", stream.id(), e);
                            return;
                        }
                        log::debug!("Stream({}) read len: {}", stream.id(), u32::from_be_bytes(len));
                        let mut buf = vec![0; u32::from_be_bytes(len) as usize];
                        if let Err(e) = stream.read_exact2(&mut buf).await {
                            log::error!("{} read failed: {:?}", stream.id(), e);
                            return;
                        }
                        log::debug!("Stream({}) read buf: {}", stream.id(), buf.len());
                        if let Err(e) = stream.write_all2(&buf).await {
                            log::error!("{} write failed: {:?}", stream.id(), e);
                            return;
                        }
                        log::debug!("Stream({}) close", stream.id());
                        if let Err(e) = stream.close2().await {
                            log::error!("{} close failed: {:?}", stream.id(), e);
                            return;
                        }
                    });
                }
            })
            .await;
            break;
        }
    })
}
