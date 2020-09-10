use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use futures::{channel, prelude::*, TryStreamExt};
use libp2p_traits::{Read2, Write2};
use mplex::connection::{self, Connection};

#[test]
fn multi_stream() {
    task::block_on(async {
        let (sender, receiver) = channel::oneshot::channel::<bytes::BytesMut>();
        let (addr_sender, addr_receiver) = channel::oneshot::channel::<::std::net::SocketAddr>();

        // server
        task::spawn(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let listener_addr = listener.local_addr().unwrap();
            let _res = addr_sender.send(listener_addr);
            let (socket, _) = listener.accept().await.unwrap();
            let _ = connection::into_stream(Connection::new(socket))
                .try_for_each_concurrent(None, |mut stream| async move {
                    let mut buf = [0; 4096];
                    loop {
                        let n = match stream.read2(&mut buf).await {
                            Ok(num) => num,
                            Err(e) => {
                                break;
                            }
                        };
                        if let Err(e) = stream.write_all2(buf[..n].as_ref()).await {
                            break;
                        };
                    }
                    Ok(())
                })
                .await;
        });

        // client
        let listener_addr = addr_receiver.await.unwrap();
        let socket = TcpStream::connect(&listener_addr).await.unwrap();

        let conn = Connection::new(socket);
        let mut ctrl = conn.control().expect("get control failed");

        task::spawn(connection::into_stream(conn).for_each(|_| future::ready(())));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let mut stream = ctrl.clone().open_stream().await.unwrap();
            let handle = task::spawn(async move {
                let data = b"hello world";
                stream.write_all2(data.as_ref()).await.unwrap();

                let mut frame = vec![0; data.len()];
                stream.read_exact2(&mut frame).await.unwrap();
                assert_eq!(&data[..], &frame[..]);

                stream.close2().await.expect("close stream");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await;
        }

        ctrl.close().await.expect("close connection");
    });
}
