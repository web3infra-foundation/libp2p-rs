use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use env_logger;
use log::info;
use secio::{handshake::Config, SecioKeyPair};

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
    let key = SecioKeyPair::secp256k1_generated();
    let config = Config::new(key);

    task::block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:1337").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut handle, _, _) = config.handshake(socket).await.unwrap();

                info!("session started!");

                let mut buf = [0; 100];

                loop {
                    if let Ok(n) = handle.read2(&mut buf).await {
                        if handle.write2(&buf[..n]).await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                info!("session closed!");
                let _ = handle.close2().await;

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
        match handle.write2(data.as_ref()).await {
            Ok(_) => info!("send all"),
            Err(e) => info!("err: {:?}", e),
        }

        let mut buf = [0; 100];
        let n = handle.read2(&mut buf).await.unwrap();
        info!("receive: {:?}", &buf[..n]);
    });
}
