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

use libp2prs_core::identity;
use libp2prs_noise::NoiseConfig;
use libp2prs_noise::{Keypair, X25519};
use libp2prs_runtime::{
    net::{TcpListener, TcpStream},
    task,
};
use libp2prs_traits::{ReadEx, WriteEx};
use log::info;

#[test]
fn test_mode_xx() {
    task::block_on(async {
        // server
        task::spawn(async {
            let server_id = identity::Keypair::generate_ed25519();
            let server_auth = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
            let server_config = NoiseConfig::xx(server_auth, server_id);

            let listener = TcpListener::bind("127.0.0.1:9876").await.unwrap();
            while let Ok((socket, _)) = listener.accept().await {
                let cfg = server_config.clone();
                let (_, mut output) = cfg.handshake(socket, false).await.unwrap();
                info!("handshake finished");
                let mut buf = [0; 100];
                let _ = output.read2(&mut buf).await;
                info!("data is {:?}", buf.to_vec());
                let u = b"!";
                buf[11] = u[0];
                let _ = output.write_all2(&buf).await;
            }
        });

        task::sleep(std::time::Duration::from_secs(1)).await;
        let client_id = identity::Keypair::generate_ed25519();
        let client_auth = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let client_config = NoiseConfig::xx(client_auth, client_id);

        //client
        let client_socket = TcpStream::connect("127.0.0.1:9876").await.unwrap();
        let (_, mut output) = client_config.handshake(client_socket, true).await.unwrap();
        let data = b"hello world";
        let _ = output.write2(data).await;
        info!("write finished");
        let mut buf = vec![0u8; 100];
        let _ = output.read2(buf.as_mut()).await;
        info!("read finished, {:?}", String::from_utf8(buf).unwrap());
    });
}
