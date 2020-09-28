// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use async_std::{
    net::{TcpListener, TcpStream},
    task,
};
use futures::channel::oneshot;
use mplex::{
    connection::{control::Control, Connection},
    error::ConnectionError,
};
use libp2p_traits::{ReadEx, ReadExt2, WriteEx};
use quickcheck::{Arbitrary, Gen, QuickCheck, TestResult};
use rand::Rng;
use std::time::Duration;
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
                let connection = Connection::new(socket);
                repeat_echo(connection).await.expect("repeat_echo")
            };

            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
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
    QuickCheck::new().tests(1).quickcheck(prop as fn(_) -> _)
}

#[test]
fn prop_slow_reader() {
    fn prop() -> bool {
        task::block_on(async move {
            let (listener, address) = bind().await.expect("bind");

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                repeat_echo(connection).await
            };

            task::spawn(server);

            let socket = TcpStream::connect(address).await.expect("connect");
            let connection = Connection::new(socket);
            let mut control = connection.control();
            let loop_handle = task::spawn(async {
                let mut muxer_conn = connection;
                muxer_conn.next_stream().await;
                log::info!("C connection {} is closed", muxer_conn.id());
            });

            let mut stream = control.clone().open_stream().await.unwrap();
            log::info!("C: opened new stream {}", stream.id());
            let res = task::spawn(async move {
                let data = b"hello world";
                // max num of pending message is 32 now
                for _ in 0..40 {
                    stream.write_all2(data.as_ref()).await.unwrap();
                    log::info!("C: {}: wrote {} bytes", stream.id(), data.len());
                }

                // wait for all echo data
                task::sleep(Duration::from_secs(5)).await;

                // drain message, and then expect read return error
                let mut frame = vec![0; data.len()];
                for _ in 0..40 {
                    stream.read_exact2(&mut frame).await?;
                }

                Ok::<(), ConnectionError>(())
            })
            .await;

            control.close().await.expect("close connection");
            loop_handle.await;

            if res.is_err() {
                true
            } else {
                false
            }
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_half_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");
            let (tx, rx) = oneshot::channel();
            let msg = vec![0x42; 40960];
            let data = msg.clone();

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                });

                if let Ok(mut stream) = control.accept_stream().await {
                    log::info!("S accept new stream {}", stream.id());
                    rx.await.expect("S oneshot receive");
                    stream
                        .write_all2(&data)
                        .await
                        .expect("server stream write all");
                    stream.close2().await.expect("server stream close");
                }
                loop_handle.await;
            };

            // client
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                });

                let mut stream = control
                    .clone()
                    .open_stream()
                    .await
                    .expect("client open stream");
                stream.close2().await.expect("client close stream");

                if stream.write_all2(b"foo").await.is_ok() {
                    return TestResult::failed();
                }

                // notify server
                tx.send(()).expect("C oneshot send");

                let mut buf = vec![0; msg.len()];
                stream
                    .read_exact2(&mut buf)
                    .await
                    .expect("client stream read exact");

                if !msg.eq(&buf) {
                    return TestResult::failed();
                }

                control.close().await.expect("client close connection");

                loop_handle.await;

                TestResult::passed()
            };

            futures::join!(server, client).1
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_fuzz_close_connection() {
    fn prop() {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                });

                control.close().await.expect("server close connection");

                loop_handle.await;
            };

            // client
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                });

                control.close().await.expect("client close connection");

                loop_handle.await;
            };

            futures::join!(server, client);
        });
    }
    QuickCheck::new().tests(100).quickcheck(prop as fn());
}

#[test]
fn prop_closing() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                    muxer_conn.streams_length()
                });

                control
                    .clone()
                    .accept_stream()
                    .await
                    .expect("S accept stream");
                control.close().await.expect("S close connection");

                loop_handle.await
            };

            // client
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                    muxer_conn.streams_length()
                });

                control.clone().open_stream().await.expect("C open stream");
                control.close().await.expect("C close connection");

                loop_handle.await
            };

            let result = futures::join!(server, client);
            TestResult::from_bool(result.0 == 0 && result.1 == 0)
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");
            let (tx, rx) = oneshot::channel();

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                });

                let mut stream = control.accept_stream().await.expect("S accept stream");
                log::info!("S accept new stream {}", stream.id());
                rx.await.expect("S oneshot receive");
                let mut buf = vec![0; 64];
                let res_w = stream.write2(b"test").await;
                let res_r = stream.read2(&mut buf).await;

                loop_handle.await;

                if res_r.is_err() && res_w.is_err() {
                    return true;
                }
                false
            };

            // client
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                });

                let mut stream = control
                    .clone()
                    .open_stream()
                    .await
                    .expect("client open stream");
                stream.reset().await.expect("client close stream");

                let mut buf = vec![0; 64];
                let res_r = stream.read2(&mut buf).await;
                let res_w = stream.write2(b"test").await;

                // notify server
                task::sleep(Duration::from_millis(200)).await;
                tx.send(()).expect("C oneshot send");

                control.close().await.expect("C close connection");
                loop_handle.await;

                if res_r.is_err() && res_w.is_err() {
                    return true;
                }

                false
            };

            let result = futures::join!(server, client);
            TestResult::from_bool(result.0 && result.1)
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset_after_eof() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");
            let (tx, rx) = oneshot::channel();

            let server = async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                });

                let mut stream = control
                    .clone()
                    .accept_stream()
                    .await
                    .expect("S accept stream");
                log::info!("S accept new stream {}", stream.id());
                stream.close2().await.expect("S close stream");

                task::sleep(Duration::from_millis(200)).await;
                tx.send(()).expect("S oneshot send");

                let mut buf = vec![0; 64];
                let res_r = stream.read2(&mut buf).await;

                // control.close().await.expect("S close connection");
                // loop_handle.await;

                if res_r.is_err() {
                    return true;
                }

                false
            };

            // client
            let client = async {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                });

                let mut stream = control
                    .clone()
                    .open_stream()
                    .await
                    .expect("client open stream");
                rx.await.expect("C oneshot receive");

                let mut buf = vec![0; 64];
                let res_r = stream.read2(&mut buf).await;

                stream.reset().await.expect("client close stream");

                control.close().await.expect("C close connection");
                loop_handle.await;

                if res_r.is_err() {
                    return true;
                }
                false
            };

            let result = futures::join!(server, client);
            TestResult::from_bool(result.0 && result.1)
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset_after_eof1() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (listener, address) = bind().await.expect("bind");
            let sa_handle = task::spawn(async move {
                let socket = listener.accept().await.expect("accept").0;
                let connection = Connection::new(socket);
                let mut control = connection.control();

                let loop_handle = task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("S connection {} is closed", muxer_conn.id());
                });

                control
                    .clone()
                    .accept_stream()
                    .await
                    .expect("S accept stream")
            });

            let sb_handle = task::spawn(async move {
                let socket = TcpStream::connect(address).await.expect("connect");
                let connection = Connection::new(socket);
                let mut control = connection.control();
                task::spawn(async {
                    let mut muxer_conn = connection;
                    muxer_conn.next_stream().await;
                    log::info!("C connection {} is closed", muxer_conn.id());
                });

                control
                    .clone()
                    .open_stream()
                    .await
                    .expect("client open stream")
            });

            let result = futures::future::join(sa_handle, sb_handle).await;
            let mut sa = result.0;
            let mut sb = result.1;

            sa.close2().await.expect("sa close");
            task::sleep(Duration::from_millis(200)).await;

            let mut buf = vec![0; 64];
            if sb.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            sb.reset().await.expect("sb reset");
            task::sleep(Duration::from_millis(200)).await;

            let mut buf = vec![0; 64];
            if sa.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
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
            muxer_conn.next_stream().await;
            log::info!("S connection {} is closed", muxer_conn.id());
        });

        while let Ok(stream) = ctrl.accept_stream().await {
            task::spawn(async move {
                loop {
                    let (r, w) = stream.clone().split2();
                    if let Err(_e) = libp2p_traits::copy(r, w).await {
                        break;
                    }
                }
            });
        }
        log::info!("S accept stream failed, exit now");
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
        let len = msg.len();
        stream.write_all2(&msg).await?;

        let mut data = vec![0; len];
        if let Err(e) = stream.read_exact2(&mut data).await {
            log::error!("read stream err {}", e);
            return Err(e.into());
        }

        stream.close2().await?;
        result.push(data)
    }
    control.close().await?;
    Ok(result)
}
