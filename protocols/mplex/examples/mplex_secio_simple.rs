use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use log::{error, info};

use libp2p_core::identity::Keypair;
use secio::Config as SecioConfig;

use libp2p_traits::{Read2, Write2};
use mplex::connection::Connection;

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

fn run_server() {
    let key = Keypair::generate_ed25519();
    let config = SecioConfig::new(key);

    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:16789").await.unwrap();
        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut sconn, _, _) = config.handshake(socket).await.unwrap();
                let mut muxer_conn = Connection::new(sconn);
                let mut ctrl = muxer_conn.control();

                task::spawn(async {
                    let mut muxer_conn = muxer_conn;
                    while let Ok(_) = muxer_conn.next_stream().await {}
                    info!("connection is closed");
                });

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    task::spawn(async move {
                        info!("accepted new stream: {:?}", stream);
                        let mut buf = [0; 4096];

                        loop {
                            let n = match stream.read2(&mut buf).await {
                                Ok(num) => num,
                                Err(e) => {
                                    error!("S {} read failed: {:?}", stream.id(), e);
                                    break;
                                }
                            };
                            info!("S {} read {:?}", stream.id(), &buf[..n]);
                            if let Err(e) = stream.write_all2(buf[..n].as_ref()).await {
                                error!("S {} write failed: {:?}", stream.id(), e);
                                break;
                            };
                        }
                        // if let Err(e) = stream.close2().await {
                        //     error!("close failed: {:?}", e);
                        //     return;
                        // };
                    });
                }
            });
        }
    });
}

fn run_client() {
    let key = Keypair::generate_ed25519();
    let config = SecioConfig::new(key);

    task::block_on(async {
        let socket = TcpStream::connect("127.0.0.1:16789").await.unwrap();
        let (sconn, _, _) = config.handshake(socket).await.unwrap();
        let muxer_conn = Connection::new(sconn);

        let mut ctrl = muxer_conn.control();

        task::spawn(async {
            let mut muxer_conn = muxer_conn;
            while let Ok(_) = muxer_conn.next_stream().await {}
            info!("connection is closed");
        });

        let mut handles = Vec::new();
        for _ in 0_u32..3 {
            let mut stream = ctrl.clone().open_stream().await.unwrap();
            let handle = task::spawn(async move {
                info!("C: opened new stream {}", stream.id());

                let data = b"hello world";

                if let Err(e) = stream.write_all2(data.as_ref()).await {
                    error!("C: {} write failed: {:?}", stream.id(), e);
                    return;
                }
                info!("C: {}: wrote {} bytes", stream.id(), data.len());

                let mut frame = vec![0; data.len()];
                if let Err(e) = stream.read_exact2(&mut frame).await {
                    error!("C: {} read failed: {:?}", stream.id(), e);
                    return;
                }
                info!("C: {} read {:?}", stream.id(), &frame);
                // assert_eq!(&data[..], &frame[..]);

                stream.close2().await.expect("close stream");

                // wait for stream to send and recv close frame
                // task::sleep(Duration::from_secs(1)).await;
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await;
        }

        ctrl.close().await.expect("close connection");

        info!("shutdown is completed");
    });
}
