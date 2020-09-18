use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use bytesize::ByteSize;
use log::info;
use std::{
    str,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use libp2p_traits::{Read2, Write2};

use yamux::{Config, Connection, Mode};

fn main() {
    env_logger::init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

const STR: &str = "fakeu1234567890cmxcmmmmmmmmmsssmssmsmsmxcmcmcnxzlllslsllcccccsannmxmxmxmxmxmxmxmxmmsssjjkzoso.";
const LEN: usize = STR.len();

static REQC: AtomicUsize = AtomicUsize::new(0);
static RESPC: AtomicUsize = AtomicUsize::new(0);

fn reqc_incr() -> usize {
    REQC.fetch_add(1, Ordering::Relaxed)
}

fn reqc() -> usize {
    REQC.swap(0, Ordering::SeqCst)
}

fn respc_incr() -> usize {
    RESPC.fetch_add(1, Ordering::Relaxed)
}

fn respc() -> usize {
    RESPC.swap(0, Ordering::SeqCst)
}

async fn show_metric() {
    let secs = 10;
    loop {
        task::sleep(Duration::from_millis(1000 * secs)).await;
        let reqc = reqc();
        let respc = respc();
        info!(
            "{} secs req {}, resp {}; {} req/s, {}/s; {} resp/s {}/s",
            secs,
            reqc,
            respc,
            reqc as f64 / secs as f64,
            ByteSize::b(((reqc * LEN) as f64 / secs as f64) as u64).to_string_as(true),
            respc as f64 / secs as f64,
            ByteSize::b(((respc * LEN) as f64 / secs as f64) as u64).to_string_as(true),
        );
    }
}

fn run_server() {
    task::spawn(show_metric());

    task::block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            info!("accepted a socket: {:?}", socket.peer_addr());
            let conn = Connection::new(socket, Config::default(), Mode::Server);
            let mut ctrl = conn.control();
            task::spawn(async move {
                task::spawn(async {
                    let mut muxer_conn = conn;
                    while muxer_conn.next_stream().await.is_ok() {}
                    info!("connection is closed");
                });

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    info!("Server accept a stream from client: id={}", stream.id());
                    task::spawn(async move {
                        let mut data = [0u8; LEN];
                        stream.read_exact2(&mut data).await.unwrap();
                        assert_eq!(data.as_ref(), STR.as_bytes());

                        loop {
                            stream.write_all2(STR.as_bytes()).await.unwrap();
                            respc_incr();

                            stream.read_exact2(&mut data).await.unwrap();
                            reqc_incr();

                            assert_eq!(data.as_ref(), STR.as_bytes());
                        }
                    });
                }
            });
        }
    });
}

fn run_client() {
    let num = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2);

    task::block_on(async move {
        let socket = TcpStream::connect("127.0.0.1:12345").await.unwrap();
        let sa = socket.peer_addr().unwrap();
        info!("[client] connected to server: {:?}", sa);

        let conn = Connection::new(socket, Config::default(), Mode::Client);
        let ctrl = conn.control();

        task::spawn(async {
            let mut muxer_conn = conn;
            while muxer_conn.next_stream().await.is_ok() {}
            info!("connection is closed");
        });

        for _ in 0..num {
            let mut ctrl = ctrl.clone();
            task::spawn(async move {
                let mut s = ctrl.open_stream().await.unwrap();
                s.write_all2(STR.as_bytes()).await.unwrap();

                let mut data = [0u8; LEN];

                loop {
                    s.read_exact2(&mut data).await.unwrap();
                    assert_eq!(&data[..], STR.as_bytes());
                    respc_incr();

                    s.write_all2(STR.as_bytes()).await.unwrap();
                    reqc_incr();
                }
            });
        }

        show_metric().await;
    });
}
