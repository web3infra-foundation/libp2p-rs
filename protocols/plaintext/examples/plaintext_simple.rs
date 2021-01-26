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
use libp2prs_runtime::{
    net::{TcpListener, TcpStream},
    task,
};
use log::{info, LevelFilter};

use libp2prs_plaintext::PlainTextConfig;
use libp2prs_traits::{ReadEx, WriteEx};

fn main() {
    env_logger::builder().filter_level(LevelFilter::Info).init();

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
        let listener = TcpListener::bind("127.0.0.1:1337").await.unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            task::spawn(async move {
                let (mut handle, _) = config.handshake(socket).await.unwrap();

                info!("session started!");

                let mut buf = [0u8; 100];

                while let Ok(n) = handle.read2(&mut buf).await {
                    buf[11] = b"!"[0];
                    if handle.write_all2(&buf[..n + 1]).await.is_err() {
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
        let stream = TcpStream::connect("127.0.0.1:1337").await.unwrap();
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
