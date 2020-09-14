use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use futures::prelude::*;
use log::info;

use yamux::{Config, Connection, Mode};
use libp2p_traits::{Read2, Write2};

use libp2p_core::identity::Keypair;
use secio::{handshake::Config as SecioConfig};

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
                let (handle, _, _) = config.handshake(socket).await
                    .expect("handshake");
                let mut conn = Connection::new(handle, Config::default(), Mode::Server);

                while let Ok(Some(mut stream)) = conn.next_stream().await {
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

        task::spawn(yamux::into_stream(conn).for_each(|_| future::ready(())));

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
