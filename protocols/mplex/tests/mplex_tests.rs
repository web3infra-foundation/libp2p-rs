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

use async_std::task;
use futures::channel::oneshot;
use futures::{channel::mpsc, prelude::*};
use libp2prs_mplex::connection::{stream::Stream as mplex_stream, Connection};
use libp2prs_traits::{copy, ReadEx, SplitEx, WriteEx};
use quickcheck::{QuickCheck, TestResult};
use std::collections::VecDeque;
use std::io::{self, Error};
use std::time::Duration;

const TEST_COUNT: u64 = 200;

#[test]
fn prop_slow_reader() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

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

            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

            for _ in 0..40 {
                sa.write_all2(b"Hello World").await.expect("A write all");
            }

            // wait
            task::sleep(Duration::from_secs(6)).await;

            let mut buf = vec![0; 64];
            let mut err = false;
            for _ in 0..40 {
                if sb.read2(&mut buf).await.is_err() {
                    err = true;
                    break;
                }
            }

            mpa_ctrl.close().await.expect("A close connection");
            mpb_ctrl.close().await.expect("A close connection");

            handle_a.await;
            handle_b.await;

            if err {
                TestResult::passed()
            } else {
                TestResult::failed()
            }
        })
    }
    QuickCheck::new().tests(5).quickcheck(prop as fn() -> _)
}

#[test]
fn prop_basic_streams() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

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
fn prop_write_after_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

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
fn prop_p2p() {
    async fn echo(s: mplex_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    async fn send_recv(mut s: mplex_stream) -> io::Result<()> {
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
                send_recv(sa).await.expect("B send recv");
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
    async fn echo(s: mplex_stream) -> io::Result<()> {
        let r = s.clone();
        let w = s;
        if let Err(e) = copy(r, w).await {
            if e.kind() != io::ErrorKind::UnexpectedEof {
                return Err(e);
            }
        }
        Ok(())
    }
    async fn send_recv(mut s: mplex_stream) -> io::Result<()> {
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
            let stream_handle_b = task::spawn(async move {
                let sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                echo(sb).await.expect("B echo");
            });

            // A act as client
            let sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
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
fn prop_half_close() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
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
            let stream_handle_b = task::spawn(async move {
                let mut sb = mpb_ctrl1.accept_stream().await.expect("B accept stream");
                let _ = rx.await;
                sb.write_all2(&msg1).await.expect("B write all");
                sb.close2().await.expect("B close");
            });

            // A act as client
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("A open stream");

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
fn prop_fuzz_close_connection() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();

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
            let mut sa = mpa_ctrl.clone().open_stream().await.expect("client open stream");
            let mut sb = mpb_ctrl.clone().accept_stream().await.expect("S accept stream");

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
            mpb_ctrl.close().await.expect("A close connection");
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
fn prop_fuzz_close_stream() {
    fn prop() -> TestResult {
        task::block_on(async {
            let (a, b) = Endpoint::new();
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
        EndpointWriter{ outgoing }
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
