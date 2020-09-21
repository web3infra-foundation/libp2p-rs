use async_std::task;
use env_logger;
use libp2p_core::identity::Keypair;
use log::info;

use libp2p_plaintext::PlainTextConfig;
use libp2p_traits::{Read2, Write2};

fn main() {
    env_logger::init();

    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        server();
    } else {
        info!("Starting client ......");
        client();
    }
}

fn server() {
    let key = Keypair::generate_secp256k1();
    let config = PlainTextConfig::new(key);

    task::block_on(async move {
        let listener = async_std::net::TcpListener::bind("127.0.0.1:1337")
            .await
            .unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut handle, _) = config.handshake(socket).await.unwrap();

                info!("session started!");

                let mut buf = [0; 100];

                loop {
                    if let Ok(n) = handle.read2(&mut buf).await {
                        if handle.write_all2(&buf[..n]).await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                info!("session closed!");
                let _ = handle.close2().await;
            });
        }
    });
}

fn client() {
    let key = Keypair::generate_secp256k1();
    let config = PlainTextConfig::new(key);

    let data = b"hello world";

    task::block_on(async move {
        let stream = async_std::net::TcpStream::connect("127.0.0.1:1337")
            .await
            .unwrap();
        let (mut handle, _) = config.handshake(stream).await.unwrap();
        match handle.write_all2(data.as_ref()).await {
            Ok(_) => info!("send all"),
            Err(e) => info!("err: {:?}", e),
        }

        let mut buf = [0; 100];
        let n = handle.read2(&mut buf).await.unwrap();
        info!("receive: {:?}", &buf[..n]);
    });
}
