use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use bytes::BytesMut;
use env_logger;
use log::info;
use secio::{handshake::Config, SecioKeyPair};

use secio::{Read, Write};

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
    let key = SecioKeyPair::secp256k1_generated();
    let config = Config::new(key);

    task::block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:1337").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut handle, _, _) = config.handshake(socket).await.unwrap();

                loop {
                    let buf = handle.read().await.unwrap();
                    let _ = handle.write(buf.as_ref()).await.unwrap();
                }

                // let (h1, h2) = handle.split();
                // match async_std::io::copy(h1, h2).await {
                //     Ok(n) => {
                //         error!("io-copy exited @len={}", n);
                //     }
                //     Err(err) => {
                //         error!("io-copy exited @{:?}", err);
                //     }
                // }
            });
        }
    });
}

fn client() {
    let key = SecioKeyPair::secp256k1_generated();
    let config = Config::new(key);

    let data = b"hello world";

    task::block_on(async move {
        let stream = TcpStream::connect("127.0.0.1:1337").await.unwrap();
        let (mut handle, _, _) = config.handshake(stream).await.unwrap();
        match handle.write(data.to_vec().as_ref()).await {
            Ok(_) => info!("send all"),
            Err(e) => info!("err: {:?}", e),
        }
        let data = handle.read().await.unwrap();
        info!("receive: {:?}", BytesMut::from(&data[..]));
    });
}
