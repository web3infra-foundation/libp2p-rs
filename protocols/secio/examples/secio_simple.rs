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

use libp2prs_core::identity::Keypair;
use libp2prs_runtime::{net, task};
use libp2prs_secio::Config;
use log::info;

// use libp2prs_traits::{ReadEx, WriteEx};
use futures::{AsyncReadExt, AsyncWriteExt};

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
    let config = Config::new(key);

    task::block_on(async move {
        let listener = net::TcpListener::bind("127.0.0.1:1337").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut handle, _, _) = config.handshake(socket).await.unwrap();

                info!("session started!");

                let mut buf = [0; 100];

                while let Ok(n) = handle.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if handle.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    handle.flush().await.expect("expect handle flush");
                }

                info!("session closed!");
                let _ = handle.close().await;

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
    let key = Keypair::generate_secp256k1();
    let config = Config::new(key);

    let data = b"hello world";

    task::block_on(async move {
        let stream = net::TcpStream::connect("127.0.0.1:1337").await.unwrap();
        let (mut handle, _, _) = config.handshake(stream).await.unwrap();
        match handle.write_all(data.as_ref()).await {
            Ok(_) => info!("send all"),
            Err(e) => info!("err: {:?}", e),
        }
        handle.flush().await.expect("expect handle flush");

        let mut buf = [0; 100];
        let n = handle.read(&mut buf).await.unwrap();
        info!("receive: {:?}", &buf[..n]);
    });
}
