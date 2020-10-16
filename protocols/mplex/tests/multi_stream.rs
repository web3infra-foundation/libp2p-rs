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

use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use futures::channel;
use libp2p_traits::{ReadEx, WriteEx};
use mplex::connection::Connection;

#[test]
fn multi_stream() {
    task::block_on(async {
        let (addr_sender, addr_receiver) = channel::oneshot::channel::<::std::net::SocketAddr>();

        // server
        task::spawn(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let listener_addr = listener.local_addr().unwrap();
            let _res = addr_sender.send(listener_addr);
            let (socket, _) = listener.accept().await.unwrap();

            let muxer_conn = Connection::new(socket);
            let mut ctrl = muxer_conn.control();
            task::spawn(async {
                let mut muxer_conn = muxer_conn;
                while let Ok(_) = muxer_conn.next_stream().await {}
            });

            while let Ok(mut stream) = ctrl.accept_stream().await {
                task::spawn(async move {
                    let mut buf = [0; 4096];

                    loop {
                        let n = match stream.read2(&mut buf).await {
                            Ok(num) => num,
                            Err(_e) => {
                                return;
                            }
                        };
                        if let Err(_e) = stream.write_all2(buf[..n].as_ref()).await {
                            return;
                        };
                    }
                });
            }
        });

        // client
        let listener_addr = addr_receiver.await.unwrap();
        let socket = TcpStream::connect(&listener_addr).await.unwrap();

        let muxer_conn = Connection::new(socket);
        let mut ctrl = muxer_conn.control();

        task::spawn(async {
            let mut muxer_conn = muxer_conn;
            while let Ok(_) = muxer_conn.next_stream().await {}
        });

        let mut handles = Vec::new();
        for _ in 0_u32..100 {
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
