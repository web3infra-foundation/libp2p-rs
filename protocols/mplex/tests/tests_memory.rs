// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use async_std::task;
use futures::channel::oneshot;
use futures::stream::FusedStream;
use futures::{channel::mpsc, prelude::*, ready};
use libp2p_traits::{ReadEx, ReadExt2, WriteEx};
use mplex::{
    connection::{
        stream::Stream as mplex_stream,
        Connection
    },
};
use quickcheck::{QuickCheck, TestResult};
use std::collections::VecDeque;
use std::time::Duration;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

#[test]
fn prop_slow_reader() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();

            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();

            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut sb = mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            for _ in 0..40 {
                sa.write_all2(b"Hello World").await.expect("A write all");
            }

            // wait
            task::sleep(Duration::from_secs(5)).await;

            let mut buf = vec![0; 64];
            let mut i = 0;
            while i < 40 {
                if sb.read2(&mut buf).await.is_err() {
                    break;
                }
                i += 1;
            }

            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");

            handle_a.await;
            handle_b.await;

            if i == 40 {
                return TestResult::failed();
            }
            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_basic_streams() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let msg = b"Hello World";
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                sb.write_all2(msg).await.expect("B write all");
                sb.close2().await.expect("B close stream");
            });

            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut buf = vec![0; msg.len()];
            sa.read_exact2(&mut buf).await.expect("A read exact");
            if !msg.eq(buf.as_slice()) {
                return TestResult::failed();
            }
            sa.close2().await.expect("B close stream");

            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");

            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_write_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let msg = b"Hello World";
            let (tx, rx) = oneshot::channel();

            let mut mpb_ctrl1 = mpb_ctrl.clone();
            task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                sb.write_all2(msg).await.expect("B write all 1");
                sb.write_all2(msg).await.expect("B write all 2");
                sb.close2().await.expect("B close stream");
                let _ = tx.send(());
            });

            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");

            // wait for writes to complete and close to happen (and be noticed)
            let _ = rx.await;

            let mut buf = vec![0; msg.len() * 2];
            sa.read_exact2(&mut buf).await.expect("A read exact");

            // read after close should fail with EOF
            if sa.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");

            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_p2p() {
    async fn echo(mut s: mplex_stream) {
        let (r, w) = s.clone().split2();
        libp2p_traits::copy(r, w).await.expect("copy");
        s.close2().await.expect("close stream");
    }
    async fn send_recv(mut s: mplex_stream) {
        // A send and recv
        let msg = b"Hello World";
        s.write_all2(msg).await.expect("write all");

        let mut buf = vec![0; msg.len()];
        s.read_exact2(&mut buf).await.expect("read exact");
        assert_eq!(msg, buf.as_slice());

        s.close2().await.expect("close stream");
    }
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let handle_a = task::spawn(async move {
                let mut mpb_ctrl2 = mpb_ctrl1.clone();
                task::spawn(async move {
                    let sb = mpb_ctrl2.accept_stream().await.expect("B accept stream");
                    echo(sb).await;
                });
                let sb = mpb_ctrl1.open_stream().await.expect("B accept stream");
                send_recv(sb).await;
            });

            let mut mpa_ctrl1 = mpa_ctrl.clone();
            let handle_b = task::spawn(async move {
                let mut mpa_ctrl2 = mpa_ctrl1.clone();
                task::spawn(async move {
                    let sa = mpa_ctrl2.accept_stream().await.expect("accept stream");
                    echo(sa).await;
                });

                let sa = mpa_ctrl1.open_stream().await.expect("open stream");
                send_recv(sa).await;
            });

            handle_a.await;
            handle_b.await;

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_echo() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                let (r, w) = sb.clone().split2();
                libp2p_traits::copy(r, w).await.expect("B copy");
                sb.close2().await.expect("B close stream");
            });

            // A act as client
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");

            // A send and recv
            let msg = b"Hello World";
            sa.write_all2(msg).await.expect("A write all");

            let mut buf = vec![0; msg.len()];
            sa.read_exact2(&mut buf).await.expect("A read exact");
            if !msg.eq(buf.as_slice()) {
                return TestResult::failed();
            }

            sa.close2().await.expect("B close stream");

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_half_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();
            let (tx, rx) = oneshot::channel();
            let msg = vec![0x42; 40960];

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let msg1 = msg.clone();
            task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                let _ = rx.await;
                sb.write_all2(&msg1).await.expect("B write all");
                sb.close2().await.expect("B close");
            });

            // A act as client
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");

            sa.close2().await.expect("B close stream");

            if sa.write2(b"foo").await.is_ok() {
                return TestResult::failed();
            }

            let _ = tx.send(());

            let mut buf = vec![0; msg.len()];
            sa.read_exact2(&mut buf).await.expect("A read exact");
            if !msg.eq(&buf) {
                return TestResult::failed();
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_fuzz_close_connection() {
    fn prop() -> TestResult {
        task::block_on(async {
            for _ in 0..1000 {
                let (a, b) = Endpoint::new();
                let a = a.into_async_read();
                let b = b.into_async_read();

                // create connection A
                let mpa = Connection::new(a);
                let mpa_ctrl = mpa.control();
                let handle_a = task::spawn(async {
                    let mut muxer_conn = mpa;
                    let _ = muxer_conn.next_stream().await;
                    log::info!("A connection {} is closed", muxer_conn.id());
                });

                // create connection B
                let mpb = Connection::new(b);
                let mut mpb_ctrl = mpb.control();
                let handle_b = task::spawn(async {
                    let mut muxer_conn = mpb;
                    let _ = muxer_conn.next_stream().await;
                    log::info!("B connection {} is closed", muxer_conn.id());
                });

                // close connection A and B
                for _ in 0..2 {
                    let mut ctrl = mpa_ctrl.clone();
                    task::spawn(async move {
                        ctrl.close().await.expect("A close connection");
                    });
                }

                mpb_ctrl.close().await.expect("A close connection");
                handle_a.await;
                handle_b.await;
            }

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_closing() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // new stream
            mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");
            let na = handle_a.await;
            let nb = handle_b.await;

            TestResult::from_bool(na == 0 && nb == 0)
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut sb = mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            sa.reset().await.expect("A reset");

            let mut buf = vec![0; 64];
            // expect read and write fail after reset
            if sa.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }
            if sa.write2(b"test").await.is_ok() {
                return TestResult::failed();
            }

            if sb.write2(b"test").await.is_ok() {
                return TestResult::failed();
            }
            if sb.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset_after_eof() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut sb = mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            sa.close2().await.expect("sa close");

            let mut buf = vec![0; 64];
            if sb.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            sb.reset().await.expect("sb reset");

            let mut buf = vec![0; 64];
            if sa.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_open_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut sb = mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            // close stream sa and sb
            sa.close2().await.expect("A close");
            sb.close2().await.expect("B close");

            // close connection A
            mpa_ctrl.clone().close().await.expect("A close connection");

            for _ in 0..2 {
                if mpa_ctrl.clone().open_stream().await.is_ok() {
                    return TestResult::failed();
                }
            }

            mpb_ctrl.close().await.expect("A close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_read_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl
                .clone()
                .open_stream()
                .await
                .expect("client open stream");
            let mut sb = mpb_ctrl
                .clone()
                .accept_stream()
                .await
                .expect("S accept stream");

            sa.close2().await.expect("sa close");

            let mut buf = vec![0; 64];
            if sb.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_fuzz_close_stream() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let a = a.into_async_read();
            let b = b.into_async_read();
            let (tx, rx) = oneshot::channel();

            // create connection A
            let mpa = Connection::new(a);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // create connection B
            let mpb = Connection::new(b);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // new stream

            let ctrl = mpa_ctrl.clone();
            task::spawn(async move {
                let mut handles = VecDeque::new();
                for _ in 0..100 {
                    let sa = ctrl
                        .clone()
                        .open_stream()
                        .await
                        .expect("client open stream");

                    for _ in 0..2 {
                        let mut sa = sa.clone();
                        let handle = task::spawn(async move {
                            sa.close2().await.expect("sa close");
                        });
                        handles.push_back(handle);
                    }
                }

                for handle in handles {
                    handle.await;
                }

                let _ = tx.send(());
            });

            let mut streams = VecDeque::new();
            for _ in 0..100 {
                let sb = mpb_ctrl
                    .clone()
                    .accept_stream()
                    .await
                    .expect("B accept stream");
                streams.push_back(sb);
            }

            let _ = rx.await;

            for mut stream in streams {
                stream.close2().await.expect("B close stream");
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            let na = handle_a.await;
            let nb = handle_b.await;

            TestResult::from_bool(na == 0 && nb == 0)
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[derive(Debug)]
struct Endpoint {
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
}

impl Endpoint {
    fn new() -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::unbounded();
        let (tx_b, rx_b) = mpsc::unbounded();

        let a = Endpoint {
            incoming: rx_a,
            outgoing: tx_b,
        };
        let b = Endpoint {
            incoming: rx_b,
            outgoing: tx_a,
        };

        (a, b)
    }
}

impl Stream for Endpoint {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if self.incoming.is_terminated() {
            return Poll::Ready(None);
        }
        if let Some(b) = ready!(Pin::new(&mut self.incoming).poll_next(cx)) {
            return Poll::Ready(Some(Ok(b)));
        }
        Poll::Pending
    }
}

impl AsyncWrite for Endpoint {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if ready!(Pin::new(&mut self.outgoing).poll_ready(cx)).is_err() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        let n = buf.len();
        if Pin::new(&mut self.outgoing)
            .start_send(Vec::from(buf))
            .is_err()
        {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.outgoing)
            .poll_flush(cx)
            .map_err(|_| io::ErrorKind::ConnectionAborted.into())
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.outgoing)
            .poll_close(cx)
            .map_err(|_| io::ErrorKind::ConnectionAborted.into())
    }
}
