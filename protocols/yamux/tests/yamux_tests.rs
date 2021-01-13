// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
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
use futures::io::Error;
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2prs_traits::{copy, ReadEx, SplitEx, WriteEx};
use libp2prs_yamux::{
    connection::{stream::Stream as yamux_stream, Connection, Mode},
    Config,
};
use quickcheck::{QuickCheck, TestResult};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

const TEST_COUNT: u64 = 200;

#[test]
fn prop_send_data_small() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let msg = b"Hello World";
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let stream_handle_b = task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                sb.write_all2(msg).await.expect("B write all");
                sb.close2().await.expect("B close stream");
            });

            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut buf = vec![0; msg.len()];
            sa.read_exact2(&mut buf).await.expect("A read exact");
            if !msg.eq(buf.as_slice()) {
                return TestResult::failed();
            }
            sa.close2().await.expect("B close stream");

            stream_handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");

            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_send_data_large() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            const SEND_SIZE: usize = 256 * 1024 * 1024;
            const RECV_SIZE: usize = 4 * 1024;

            let msg: Vec<u8> = vec![0x42; SEND_SIZE];
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let stream_handle_b = task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                sb.write_all2(&msg).await.expect("B write all");
                sb.close2().await.expect("B close stream");
            });

            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut buf = vec![0; RECV_SIZE];

            for _ in 0..SEND_SIZE / RECV_SIZE {
                sa.read_exact2(&mut buf).await.expect("A read exact");
            }
            sa.close2().await.expect("B close stream");

            stream_handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");

            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(10).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_p2p() {
    async fn echo(s: yamux_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    async fn send_recv(mut s: yamux_stream) -> io::Result<()> {
        // A send and recv
        let msg = b"Hello World";
        s.write_all2(msg).await?;

        let mut buf = vec![0; msg.len()];
        s.read_exact2(&mut buf).await?;
        assert_eq!(msg, buf.as_slice());

        s.close2().await?;

        Ok(())
    }
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let handle_b = task::spawn(async move {
                let mut mpb_ctrl2 = mpb_ctrl1.clone();
                let handle = task::spawn(async move {
                    let sb = mpb_ctrl2.accept_stream().await.expect("B accept stream");
                    echo(sb).await.expect("B echo");
                });
                let sb = mpb_ctrl1.open_stream().await.expect("B accept stream");
                send_recv(sb).await.expect("B send recv");
                handle.await;
            });

            let mut mpa_ctrl1 = mpa_ctrl.clone();
            let handle_a = task::spawn(async move {
                let mut mpa_ctrl2 = mpa_ctrl1.clone();
                let handle = task::spawn(async move {
                    let sa = mpa_ctrl2.accept_stream().await.expect("accept stream");
                    echo(sa).await.expect("A echo");
                });

                let sa = mpa_ctrl1.open_stream().await.expect("open stream");
                send_recv(sa).await.expect("A send recv");
                handle.await;
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_accept() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                assert_eq!(mpa.streams_length(), 0);
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                assert_eq!(mpb.streams_length(), 0);
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as client
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let handle_b = task::spawn(async move {
                let mut mpb_ctrl2 = mpb_ctrl1.clone();
                let handle = task::spawn(async move {
                    let mut sb = mpb_ctrl2.accept_stream().await.expect("B accept stream");
                    assert_eq!(sb.id(), 2);
                    sb.close2().await.expect("B close stream");
                });
                let mut sb = mpb_ctrl1.open_stream().await.expect("B accept stream");
                assert_eq!(sb.id(), 1);
                sb.close2().await.expect("B close stream");

                handle.await;
            });

            // A act as server
            let mut mpa_ctrl1 = mpa_ctrl.clone();
            let handle_a = task::spawn(async move {
                let mut mpa_ctrl2 = mpa_ctrl1.clone();
                let handle = task::spawn(async move {
                    let mut sa = mpa_ctrl2.accept_stream().await.expect("accept stream");
                    assert_eq!(sa.id(), 1);
                    sa.close2().await.expect("B close stream");
                });

                let mut sa = mpa_ctrl1.open_stream().await.expect("open stream");
                assert_eq!(sa.id(), 2);
                sa.close2().await.expect("B close stream");
                handle.await;
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_echo() {
    async fn echo(s: yamux_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    async fn send_recv(mut s: yamux_stream) -> io::Result<()> {
        // A send and recv
        let msg = b"Hello World";
        s.write_all2(msg).await?;

        let mut buf = vec![0; msg.len()];
        s.read_exact2(&mut buf).await?;
        assert_eq!(msg, buf.as_slice());

        s.close2().await?;

        Ok(())
    }
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let handle_b = task::spawn(async move {
                let sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                echo(sb).await.expect("B echo");
            });

            // A act as client
            let sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");

            // A send and recv
            send_recv(sa).await.expect("A send recv");

            // close connection A and B
            handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_concurrent() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            const DATA_SIZE: usize = 100 * 1024;
            let msg = vec![0x42; DATA_SIZE];

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let handle_b = task::spawn(async move {
                let mut handles = VecDeque::default();
                while let Ok(mut sb) = mpb_ctrl1.accept_stream().await {
                    let handle = task::spawn(async move {
                        let mut buf = vec![0; DATA_SIZE];
                        // B echo
                        sb.read_exact2(&mut buf).await.expect("B read exact");
                        sb.write_all2(&buf).await.expect("B write all");
                        let _ = sb.close2().await;
                    });
                    handles.push_back(handle);
                }

                for handle in handles {
                    handle.await;
                }
            });

            let mut handles = VecDeque::default();
            // A act as client
            for _ in 0..10000 {
                let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
                let msg = msg.clone();
                let handle = task::spawn(async move {
                    // A send and recv
                    sa.write_all2(&msg).await.expect("A write all");

                    let mut buf = vec![0; msg.len()];
                    sa.read_exact2(&mut buf).await.expect("A read exact");

                    sa.close2().await.expect("A close stream");
                });
                handles.push_back(handle);
            }

            for handle in handles {
                handle.await;
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            handle_b.await;
            loop_handle_a.await;
            loop_handle_b.await;

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
            let (tx, rx) = oneshot::channel();
            let msg = vec![0x42; 40960];

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let msg1 = msg.clone();
            let handle_b = task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                let _ = rx.await;
                sb.write_all2(&msg1).await.expect("B write all");
                sb.close2().await.expect("B close");
            });

            // A act as client
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");

            sa.close2().await.expect("A close stream");

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
            handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_half_close2() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let msg = vec![0x42; 40960];

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // A act as client
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            sa.write_all2(&msg).await.expect("A write all");

            // B act as server
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("B accept stream");

            sb.close2().await.expect("B close stream");

            let mut buf = vec![0; msg.len()];
            sb.read_exact2(&mut buf).await.expect("B read exact");
            if !msg.eq(&buf) {
                return TestResult::failed();
            }

            // send more
            if sa.write2(&msg).await.is_err() {
                return TestResult::failed();
            }
            sa.close2().await.expect("A close stream");

            // read after eof
            let mut buf = vec![0; msg.len()];
            sb.read_exact2(&mut buf).await.expect("B read exact");
            if !msg.eq(&buf) {
                return TestResult::failed();
            }

            // EOF after close
            let mut buf = vec![0; msg.len()];
            if sb.read_exact2(&mut buf).await.is_ok() {
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_lazy_open() {
    async fn echo(s: yamux_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    async fn send_recv(mut s: yamux_stream) -> io::Result<()> {
        // A send and recv
        let msg = b"Hello World";
        s.write_all2(msg).await?;

        let mut buf = vec![0; msg.len()];
        s.read_exact2(&mut buf).await?;
        assert_eq!(msg, buf.as_slice());

        s.close2().await?;

        Ok(())
    }
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let mut config = Config::default();
            config.set_lazy_open(true);

            // create connection A
            let mpa = Connection::new(a, config, Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let stream_handle_b = task::spawn(async move {
                let sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                echo(sb).await.expect("B echo");
            });

            // A act as client
            let sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");

            // A send and recv
            send_recv(sa).await.expect("A send recv");

            // close connection A and B
            stream_handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_cannot_read_after_close() {
    async fn echo(s: yamux_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let mut config = Config::default();
            config.set_read_after_close(false);
            let config_b = config.clone();

            // create connection A
            let mpa = Connection::new(a, config, Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, config_b, Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // B act as server
            let mut mpb_ctrl1 = mpb_ctrl.clone();
            let stream_handle_b = task::spawn(async move {
                let sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                echo(sb).await.expect("B echo");
            });

            // A act as client
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");

            // A send and recv
            let msg = b"Hello World";
            sa.write_all2(msg).await.expect("A write all");

            sa.close2().await.expect("A close stream");

            let mut buf = vec![0; msg.len()];
            if sa.read_exact2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }

            // close connection A and B
            stream_handle_b.await;
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_client_client() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Client);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            mpa_ctrl.clone().open_stream().await.expect("A open stream");
            if mpb_ctrl.clone().accept_stream().await.is_ok() {
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_server_server() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Server);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            mpa_ctrl.clone().open_stream().await.expect("A open stream");
            if mpb_ctrl.clone().accept_stream().await.is_ok() {
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_close_before_ack() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            for _ in 0..8 {
                let mut sa = mpa_ctrl.clone().open_stream().await.expect("A open stream");
                sa.close2().await.expect("A close stream");
            }

            for _ in 0..8 {
                let mut sb = mpb_ctrl.clone().accept_stream().await.expect("B accept stream");
                sb.close2().await.expect("B close stream");
            }

            // close stream before ack
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("A open stream");
            sa.close2().await.expect("A close stream");

            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");

            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_stream_reset_after_conn_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            task::spawn(async move {
                mpa_ctrl.close().await.expect("A close connection");
            });

            // new stream
            if let Ok(mut sb) = mpb_ctrl.clone().open_stream().await {
                let _ = sb.reset().await;
            }

            // close connection A and B
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_stream_close_after_conn_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            task::spawn(async move {
                mpa_ctrl.close().await.expect("A close connection");
            });

            // new stream
            if let Ok(mut sb) = mpb_ctrl.clone().open_stream().await {
                let _ = sb.close2().await;
                let _ = sb.reset().await;
            }

            // close connection A and B
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_stream_write_after_conn_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            task::spawn(async move {
                mpa_ctrl.close().await.expect("A close connection");
            });

            // new stream
            if let Ok(mut sb) = mpb_ctrl.clone().open_stream().await {
                let _ = sb.write2(b"Hello World").await;
                let _ = sb.reset().await;
            }

            // close connection A and B
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_stream_read_after_conn_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            task::spawn(async move {
                mpa_ctrl.close().await.expect("A close connection");
            });

            // new stream
            if let Ok(mut sb) = mpb_ctrl.clone().open_stream().await {
                let mut buf = vec![0; 64];
                let _ = sb.read2(&mut buf).await;
                let _ = sb.reset().await;
            }

            // close connection A and B
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            TestResult::passed()
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_goaway() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            mpa_ctrl.close().await.expect("A close connection");

            // new stream
            let mut remote_goaway = false;
            for _ in 0..100 {
                if let Ok(mut sb) = mpb_ctrl.clone().open_stream().await {
                    let _ = sb.close2().await;
                } else {
                    remote_goaway = true;
                    break;
                }
            }

            // close connection A and B
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            if remote_goaway {
                TestResult::passed()
            } else {
                TestResult::failed()
            }
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_max_streams() {
    fn prop(n: usize) -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            let max_streams = n % 100;
            let mut cfg = Config::default();
            cfg.set_max_num_streams(max_streams);

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, cfg, Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut v = Vec::default();
            for _ in 0..max_streams {
                v.push(mpb_ctrl.clone().open_stream().await.expect("B open stream"));
            }

            let mut max_stream_err = false;
            if mpb_ctrl.clone().open_stream().await.is_err() {
                max_stream_err = true;
            }

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            handle_a.await;
            handle_b.await;

            if max_stream_err {
                TestResult::passed()
            } else {
                TestResult::failed()
            }
        })
    }

    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn(_) -> _)
}

#[test]
fn prop_stream_reset_write() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");
            sb.reset().await.expect("A reset");
            task::sleep(Duration::from_millis(100)).await;
            let err = sa.write_all2(b"Hello World").await.is_ok();

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("B close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            if err {
                TestResult::failed()
            } else {
                TestResult::passed()
            }
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_stream_reset_read() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

            let mut sa1 = sa.clone();
            let handle_a = task::spawn(async move {
                let mut buf = vec![0; 64];
                if sa1.read2(&mut buf).await.is_ok() {
                    TestResult::failed()
                } else {
                    TestResult::passed()
                }
            });

            let handle_b = task::spawn(async move {
                let mut buf = vec![0; 64];
                if sb.read2(&mut buf).await.is_ok() {
                    TestResult::failed()
                } else {
                    TestResult::passed()
                }
            });

            sa.reset().await.expect("A reset");

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            let result = futures::future::join(handle_a, handle_b).await;
            if result.0.is_failure() || result.1.is_failure() {
                TestResult::failed()
            } else {
                TestResult::passed()
            }
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_accept_stream_after_close_conn() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let loop_handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mpb_ctrl = mpb.control();
            let loop_handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            mpa_ctrl.clone().open_stream().await.expect("client open stream");

            let mut mpb_ctrl1 = mpb_ctrl.clone();
            task::spawn(async move {
                while let Ok(_stream) = mpb_ctrl1.clone().accept_stream().await {
                    mpb_ctrl1.close().await.expect("A close connection");
                    task::sleep(Duration::from_secs(1)).await;
                }
            })
            .await;

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            loop_handle_a.await;
            loop_handle_b.await;

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_fuzz_close_connection() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
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

            TestResult::passed()
        })
    }
    QuickCheck::new().tests(1000).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_closing() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // new stream
            mpa_ctrl.clone().open_stream().await.expect("client open stream");
            mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

            // close connection A and B
            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");
            let na = handle_a.await;
            let nb = handle_b.await;

            TestResult::from_bool(na == 0 && nb == 0)
        })
    }
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("A open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("B accept stream");

            sa.reset().await.expect("A reset");

            let mut buf = vec![0; 64];
            // expect read and write fail after reset
            if sa.read2(&mut buf).await.is_ok() {
                return TestResult::failed();
            }
            if sa.write2(b"test").await.is_ok() {
                return TestResult::failed();
            }

            task::sleep(Duration::from_millis(200)).await;

            if sb.write2(b"test").await.is_ok() {
                return TestResult::failed();
            }
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_reset_after_eof() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_open_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_read_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
            let mut mpb_ctrl = mpb.control();
            let handle_b = task::spawn(async {
                let mut muxer_conn = mpb;
                let _ = muxer_conn.next_stream().await;
                log::info!("B connection {} is closed", muxer_conn.id());
            });

            // new stream
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_read_after_close2() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
            });

            let mpb = Connection::new(b, Config::default(), Mode::Client);
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

            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");

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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_fuzz_close_stream() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
            let (tx, rx) = oneshot::channel();

            // create connection A
            let mpa = Connection::new(a, Config::default(), Mode::Server);
            let mut mpa_ctrl = mpa.control();
            let handle_a = task::spawn(async {
                let mut muxer_conn = mpa;
                let _ = muxer_conn.next_stream().await;
                log::info!("A connection {} is closed", muxer_conn.id());
                muxer_conn.streams_length()
            });

            // create connection B
            let mpb = Connection::new(b, Config::default(), Mode::Client);
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
                    let sa = ctrl.clone().open_stream().await.expect("client open stream");

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
                let sb = mpb_ctrl.clone().accept_stream().await.expect("B accept stream");
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
    QuickCheck::new().tests(TEST_COUNT).quickcheck(prop as fn() -> _)
}

#[derive(Debug)]
struct Endpoint {
    incoming: EndpointReader,
    outgoing: EndpointWriter,
}

#[derive(Debug)]
struct EndpointReader {
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
    recv_buf: Vec<u8>,
}

impl EndpointReader {
    fn new(incoming: mpsc::UnboundedReceiver<Vec<u8>>) -> Self {
        EndpointReader {
            incoming,
            recv_buf: Vec::default(),
        }
    }

    #[inline]
    fn drain(&mut self, buf: &mut [u8]) -> usize {
        // Return zero if there is no data remaining in the internal buffer.
        if self.recv_buf.is_empty() {
            return 0;
        }

        // calculate number of bytes that we can copy
        let n = ::std::cmp::min(buf.len(), self.recv_buf.len());

        // Copy data to the output buffer
        buf[..n].copy_from_slice(self.recv_buf[..n].as_ref());

        // drain n bytes of recv_buf
        self.recv_buf = self.recv_buf.split_off(n);

        n
    }
}

#[async_trait]
impl ReadEx for EndpointReader {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let copied = self.drain(buf);
        if copied > 0 {
            log::debug!("drain recv buffer data size: {:?}", copied);
            return Ok(copied);
        }
        let t = self
            .incoming
            .next()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "channel closed"))?;

        let n = t.len();
        if buf.len() >= n {
            buf[..n].copy_from_slice(t.as_ref());
            Ok(n)
        } else {
            // fill internal recv buffer
            self.recv_buf = t;
            // drain for input buffer
            let copied = self.drain(buf);
            Ok(copied)
        }
    }
}

#[derive(Debug)]
struct EndpointWriter {
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
}

impl EndpointWriter {
    fn new(outgoing: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        EndpointWriter { outgoing }
    }
}

#[async_trait]
impl WriteEx for EndpointWriter {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        let n = buf.len();
        self.outgoing
            .send(Vec::from(buf))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "send failure"))?;
        Ok(n)
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        Ok(())
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.outgoing.close_channel();
        Ok(())
    }
}

impl Endpoint {
    fn new() -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::unbounded();
        let (tx_b, rx_b) = mpsc::unbounded();

        let a = Endpoint {
            incoming: EndpointReader::new(rx_a),
            outgoing: EndpointWriter::new(tx_b),
        };
        let b = Endpoint {
            incoming: EndpointReader::new(rx_b),
            outgoing: EndpointWriter::new(tx_a),
        };
        (a, b)
    }
}

use async_trait::async_trait;

#[async_trait]
impl ReadEx for Endpoint {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.incoming.read2(buf).await
    }
}

#[async_trait]
impl WriteEx for Endpoint {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.outgoing.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        self.outgoing.flush2().await
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.outgoing.flush2().await
    }
}

impl SplitEx for Endpoint {
    type Reader = EndpointReader;
    type Writer = EndpointWriter;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (self.incoming, self.outgoing)
    }
}

/*
impl Stream for Endpoint {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut recver = futures::ready!(self.incoming.lock().poll_unpin(cx));
        if recver.is_terminated() {
            return Poll::Ready(None);
        }
        if let Some(b) = ready!(Pin::new(&mut recver).poll_next(cx)) {
            return Poll::Ready(Some(Ok(b)));
        }
        Poll::Pending
    }
}

impl AsyncWrite for Endpoint {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        if ready!(Pin::new(&mut self.outgoing).poll_ready(cx)).is_err() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        let n = buf.len();
        if Pin::new(&mut self.outgoing).start_send(Vec::from(buf)).is_err() {
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
 */
