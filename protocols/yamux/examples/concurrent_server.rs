use async_std::{
    net::{TcpListener},
    task,
};
use libp2p_traits::{ReadEx, WriteEx};
use yamux::{Config, Connection, Mode};
use std::collections::HashSet;

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
            }).await;
            break;
        }
    })
}
