use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use libp2p_traits::{Read2, Write2};
use log::{error, info};
use mplex::connection::Connection;
use std::collections::VecDeque;
use std::sync::Arc;

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

fn run_server() {
    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:8088").await.unwrap();
        while let Ok((socket, _)) = listener.accept().await {
            task::spawn(async move {
                let muxer_conn = Connection::new(socket);
                let mut ctrl = muxer_conn.control();

                task::spawn(async {
                    let mut muxer_conn = muxer_conn;
                    let _ = muxer_conn.next_stream().await;
                    info!("connection is closed");
                });

                while let Ok(mut stream) = ctrl.accept_stream().await {
                    info!("accepted new stream: {:?}", stream);
                    task::spawn(async move {
                        let mut len = [0; 4];
                        if let Err(e) = stream.read_exact2(&mut len).await {
                            error!("{} read failed: {:?}", stream.id(), e);
                            return;
                        }
                        let mut buf = vec![0; u32::from_be_bytes(len) as usize];
                        if let Err(e) = stream.read_exact2(&mut buf).await {
                            error!("{} read failed: {:?}", stream.id(), e);
                            return;
                        }
                        if let Err(e) = stream.write_all2(&buf).await {
                            error!("{} write failed: {:?}", stream.id(), e);
                            return;
                        }
                        if let Err(e) = stream.close2().await {
                            error!("{} close failed: {:?}", stream.id(), e);
                            return;
                        }
                    });
                }
            });
        }
    });
}

fn run_client() {
    task::block_on(async {
        let socket = TcpStream::connect("127.0.0.1:8088").await.unwrap();
        let muxer_conn = Connection::new(socket);

        let mut ctrl = muxer_conn.control();

        let loop_handle = task::spawn(async {
            let mut muxer_conn = muxer_conn;
            let _ = muxer_conn.next_stream().await;
            info!("connection is closed");
        });

        let mut handles = VecDeque::new();
        let data = Arc::new(vec![0x42; 100 * 1024]);
        for _ in 0..10 {
            let mut stream = ctrl.clone().open_stream().await.unwrap();
            let data = data.clone();
            info!("C: opened new stream {}", stream.id());
            let handle = task::spawn(async move {
                stream
                    .write_all2(&(data.len() as u32).to_be_bytes()[..])
                    .await
                    .unwrap();

                stream.write_all2(data.as_ref()).await.unwrap();
                info!("C: {}: wrote {} bytes", stream.id(), data.len());

                let mut frame = vec![0; data.len()];
                stream.read_exact2(&mut frame).await.unwrap();
                assert_eq!(&data[..], &frame[..]);

                stream.close2().await.expect("close stream");

                // wait for stream to send and recv close frame
                // task::sleep(Duration::from_secs(1)).await;
            });
            handles.push_back(handle);
        }

        while let Some(handle) = handles.pop_front() {
            handle.await;
        }

        ctrl.close().await.expect("close connection");

        loop_handle.await;

        info!("shutdown is completed");
    });
}
