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

use futures::{AsyncReadExt, AsyncWriteExt};
use libp2prs_runtime::task;
use log::info;

use libp2prs_core::Transport;
use libp2prs_tcp::TcpConfig;
use libp2prs_yamux::Config as YamuxConfig;

use libp2prs_core::identity::Keypair;
use libp2prs_core::muxing::StreamMuxer;
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use libp2prs_secio::Config as SecioConfig;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

#[allow(clippy::clone_double_ref)]
fn run_server() {
    task::block_on(async {
        let mut listener = TcpConfig::new()
            .listen_on("/ip4/127.0.0.1/tcp/12345".parse().unwrap())
            .expect("listen on");

        while let Ok(socket) = listener.accept_output().await {
            task::spawn(async move {
                let key = Keypair::generate_secp256k1();
                let cfg = SecioConfig::new(key);
                let info = cfg.protocol_info().get(0).unwrap().clone();
                let socket = cfg.upgrade_inbound(socket, info).await.expect("secio upgrade inbound");

                let cfg = YamuxConfig::server();
                let info = cfg.protocol_info().get(0).unwrap().clone();

                let mut stream_muxer = cfg.upgrade_outbound(socket, info).await.expect("yamux upgrade inbound");

                if let Some(task) = stream_muxer.task() {
                    task::spawn(async move {
                        task.await;
                    });
                }

                while let Ok(mut s) = stream_muxer.accept_stream().await {
                    task::spawn(async move {
                        let mut buf = [0u8; 1024];
                        loop {
                            let n = s.read(&mut buf).await.expect("read2");
                            if n == 0 {
                                break;
                            }
                            s.write_all(&buf[..n]).await.expect("write all");
                            s.flush().await.unwrap();
                        }
                    });
                }
                info!("connection is closed");
            });
        }
    });
}

#[allow(clippy::clone_double_ref)]
fn run_client() {
    task::block_on(async move {
        let socket = TcpConfig::new()
            .dial("/ip4/127.0.0.1/tcp/12345".parse().unwrap())
            .await
            .expect("dial");

        let key = Keypair::generate_secp256k1();

        let cfg = SecioConfig::new(key);
        let info = cfg.protocol_info().get(0).unwrap().clone();

        let socket = cfg.upgrade_outbound(socket, info).await.expect("secio upgrade outbound");

        let cfg = YamuxConfig::client();
        let info = cfg.protocol_info().get(0).unwrap().clone();

        let mut stream_muxer = cfg.upgrade_outbound(socket, info).await.expect("yamux upgrade outbound");

        if let Some(task) = stream_muxer.task() {
            task::spawn(async move {
                task.await;
            });
        }

        let mut stream = stream_muxer.open_stream().await.expect("open stream");

        let data = b"hello world";

        stream.write_all(data).await.unwrap();
        info!("C: {}: wrote {} bytes", stream.id(), data.len());
        // need flush?
        stream.flush().await.unwrap();

        let mut frame = vec![0; data.len()];
        stream.read_exact(&mut frame).await.unwrap();

        info!("C: {}: read {} bytes", stream.id(), frame.len());

        assert_eq!(&data[..], &frame[..]);

        let _ = stream.close().await;

        //Ok::<(), yamux::ConnectionError>(())
    });
}
