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

use crate::{connection::State, Config, Connection, ConnectionError, Control, Mode};
use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use libp2prs_traits::{ReadEx, ReadExt2, WriteEx};
use quickcheck::{Arbitrary, Gen, QuickCheck, TestResult};
use rand::Rng;
use std::{
    fmt::Debug,
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
};

#[test]
fn prop_send_recv() {
    fn prop(msgs: Vec<Msg>) -> TestResult {
        if msgs.is_empty() {
            return TestResult::discard();
        }
        task::block_on(async move {
            let num_requests = msgs.len();
            let iter = msgs.into_iter().map(|m| m.0);

            let (listener, address) = bind().await.expect("bind");

            let server = async {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket, Config::default(), Mode::Server);
                repeat_echo(connection).await.expect("repeat_echo")
            };

            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket, Config::default(), Mode::Client);
                let control = connection.control();

                task::spawn(async {
                    let mut muxer_conn = connection;
                    while muxer_conn.next_stream().await.is_ok() {}
                    log::info!("connection is closed");
                });

                send_recv(control, iter.clone()).await.expect("send_recv")
            };

            let result = futures::join!(server, client).1;
            TestResult::from_bool(result.len() == num_requests && result.into_iter().eq(iter))
        })
    }
    // env_logger::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    QuickCheck::new().tests(1).quickcheck(prop as fn(_) -> _)
}

#[test]
fn prop_max_streams() {
    fn prop(n: usize) -> bool {
        let max_streams = n % 100;
        log::error!("test for max streams ({})", max_streams);
        let mut cfg = Config::default();
        cfg.set_max_num_streams(max_streams);

        task::block_on(async move {
            let (listener, address) = bind().await.expect("bind");

            let cfg_s = cfg.clone();
            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket, cfg_s, Mode::Server);
                repeat_echo(connection).await
            };

            task::spawn(server);

            let socket = TcpStream::connect(address).await.expect("connect");
            let connection = Connection::new(socket, cfg, Mode::Client);
            let mut control = connection.control();
            task::spawn(async {
                let mut muxer_conn = connection;
                while muxer_conn.next_stream().await.is_ok() {}
                log::info!("connection is closed");
            });

            let mut v = Vec::new();
            for _ in 0..max_streams {
                v.push(control.open_stream().await.expect("open_stream"))
            }
            if let Err(ConnectionError::TooManyStreams) = control.open_stream().await {
                true
            } else {
                false
            }
        })
    }
    QuickCheck::new().tests(7).quickcheck(prop as fn(_) -> _)
}

#[test]
fn prop_send_recv_half_closed() {
    fn prop(msg: Msg) {
        let msg_len = msg.0.len();
        log::info!("prop_send_recv_half_closed msg len ({})", msg_len);
        task::block_on(async move {
            let (listener, address) = bind().await.expect("bind");

            // Server should be able to write on a stream shutdown by the client.
            let server = async {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket, Config::default(), Mode::Server);
                let mut ctrl = connection.control();
                task::spawn(async {
                    let mut muxer_conn = connection;
                    while muxer_conn.next_stream().await.is_ok() {}
                    log::info!("connection is closed");
                });
                if let Ok(mut stream) = ctrl.accept_stream().await {
                    let mut buf = vec![0; msg_len];
                    stream.read_exact2(&mut buf).await.expect("S: read_exact");
                    stream.write_all2(&buf).await.expect("S: send");
                    stream.close2().await.expect("S: close")
                }
            };

            // Client should be able to read after shutting down the stream.
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket, Config::default(), Mode::Client);
                let mut control = connection.control();
                task::spawn(async {
                    let mut muxer_conn = connection;
                    while muxer_conn.next_stream().await.is_ok() {}
                    log::info!("connection is closed");
                });

                let mut stream = control.open_stream().await.expect("C: open_stream");
                stream.write_all2(&msg.0).await.expect("C: send");
                stream.close2().await.expect("C: close");
                assert_eq!(State::SendClosed, stream.state());
                let mut buf = vec![0; msg_len];
                stream.read_exact2(&mut buf).await.expect("C: read_exact");
                assert_eq!(buf, msg.0);
                assert_eq!(Some(0), stream.read2(&mut buf).await.ok());
                assert_eq!(State::Closed, stream.state());
            };

            futures::join!(server, client);
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn(_))
}

#[derive(Clone, Debug)]
struct Msg(Vec<u8>);

impl Arbitrary for Msg {
    fn arbitrary<G: Gen>(g: &mut G) -> Msg {
        let n: usize = g.gen_range(1, g.size() + 1);
        let mut v = vec![0; n];
        g.fill(&mut v[..]);
        Msg(v)
    }
}

async fn bind() -> io::Result<(TcpListener, SocketAddr)> {
    let i = Ipv4Addr::new(127, 0, 0, 1);
    let s = SocketAddr::V4(SocketAddrV4::new(i, 0));
    let l = TcpListener::bind(&s).await?;
    let a = l.local_addr()?;
    Ok((l, a))
}

/// For each incoming stream of `c` echo back to the sender.
async fn repeat_echo(c: Connection<TcpStream>) -> Result<(), ConnectionError> {
    let mut ctrl = c.control();
    task::spawn(async move {
        task::spawn(async {
            let mut muxer_conn = c;
            while muxer_conn.next_stream().await.is_ok() {}
            log::info!("connection is closed");
        });

        while let Ok(mut stream) = ctrl.accept_stream().await {
            task::spawn(async move {
                let (r, w) = stream.clone().split2();
                libp2prs_traits::copy(r, w).await.unwrap();
                stream.close2().await.unwrap();
            });
        }
    })
    .await;
    Ok(())
}

/// For each message in `iter`, open a new stream, send the message and
/// collect the response. The sequence of responses will be returned.
async fn send_recv<I>(mut control: Control, iter: I) -> Result<Vec<Vec<u8>>, ConnectionError>
where
    I: Iterator<Item = Vec<u8>>,
{
    let mut result = Vec::new();
    for msg in iter {
        let mut stream = control.open_stream().await?;
        log::debug!("C: new stream: {}", stream);
        let id = stream.id();
        let len = msg.len();
        stream.write_all2(&msg).await?;
        log::debug!("C: {}: sent {} bytes", id, len);
        stream.close2().await?;
        log::debug!("stream({}) closed", id);
        let mut data = vec![0; len];
        let x = stream.read_exact2(&mut data).await;
        match x {
            Err(e) => {
                log::error!("read from closed stream err {}", e);
                return Err(e.into());
            }
            Ok(n) => {
                log::debug!("==> read after close {:?}", n);
            }
        }
        log::debug!("C: {}: received {} bytes", id, data.len());
        result.push(data)
    }
    log::debug!("C: closing connection");
    control.close().await?;
    Ok(result)
}
