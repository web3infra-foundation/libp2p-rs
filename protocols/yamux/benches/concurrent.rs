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
use criterion::{criterion_group, criterion_main, Criterion};
use futures::stream::FusedStream;
use futures::{channel::mpsc, prelude::*, ready};
use libp2prs_traits::{ReadEx, WriteEx};
use libp2prs_yamux::{connection::Connection, connection::Mode, error::ConnectionError, Config};
use std::collections::VecDeque;
use std::{
    fmt, io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

criterion_group!(benches, concurrent);
criterion_main!(benches);

#[derive(Copy, Clone)]
struct Params {
    streams: usize,
    messages: usize,
}

impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "((streams {}) (messages {}))", self.streams, self.messages)
    }
}

#[derive(Debug, Clone)]
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

fn concurrent(c: &mut Criterion) {
    env_logger::from_env(env_logger::Env::default().default_filter_or("error")).init();
    let params = &[
        Params { streams: 1, messages: 1 },
        Params { streams: 10, messages: 1 },
        Params { streams: 1, messages: 10 },
        Params { streams: 100, messages: 1 },
        Params { streams: 1, messages: 100 },
        Params {
            streams: 10,
            messages: 100,
        },
        Params {
            streams: 100,
            messages: 10,
        },
    ];

    let data0 = Bytes(Arc::new(vec![0x42; 4096]));
    let data1 = data0.clone();
    let data2 = data0.clone();

    c.bench_function_over_inputs(
        "one by one",
        move |b, &&params| {
            let data = data1.clone();
            b.iter(move || task::block_on(roundtrip(params.streams, params.messages, data.clone(), false)))
        },
        params,
    );

    c.bench_function_over_inputs(
        "all at once",
        move |b, &&params| {
            let data = data2.clone();
            b.iter(move || task::block_on(roundtrip(params.streams, params.messages, data.clone(), true)))
        },
        params,
    );
}

#[allow(dead_code)]
async fn roundtrip(nstreams: usize, nmessages: usize, data: Bytes, send_all: bool) {
    let data_len = data.0.len();
    let (server, client) = Endpoint::new();
    let server = server.into_async_read();
    let client = client.into_async_read();

    let conn = Connection::new(server, Config::default(), Mode::Server);
    let ctrl_server = conn.control();
    let loop_handle_server = task::spawn(async {
        let mut muxer_conn = conn;
        while muxer_conn.next_stream().await.is_ok() {}
        log::info!("S connection is closed");
    });

    let mut ctrl = ctrl_server.clone();
    let stream_handle_server = task::spawn(async move {
        let mut handles = VecDeque::new();
        while let Ok(mut stream) = ctrl.accept_stream().await {
            log::debug!("S: accepted new stream");
            let handle = task::spawn(async move {
                let mut buf = vec![0; data_len];
                for _ in 0..nmessages {
                    stream.read_exact2(&mut buf).await?;
                    stream.write_all2(&buf).await?;
                }
                stream.close2().await?;
                Ok::<(), ConnectionError>(())
            });
            handles.push_back(handle);
        }

        for handle in handles {
            handle.await.expect("server stream task");
        }
    });

    let conn = Connection::new(client, Config::default(), Mode::Client);
    let mut ctrl_client = conn.control();
    let loop_handle_client = task::spawn(async {
        let mut muxer_conn = conn;
        while muxer_conn.next_stream().await.is_ok() {}
        log::info!("C connection is closed");
    });

    let (tx, rx) = mpsc::unbounded();
    for _ in 0..nstreams {
        let data = data.0.clone();
        let tx = tx.clone();
        let mut ctrl = ctrl_client.clone();
        task::spawn(async move {
            let mut stream = ctrl.open_stream().await?;
            if send_all {
                for _ in 0..nmessages {
                    stream.write_all2(&data).await?;
                }

                stream.close2().await?;

                for _ in 0..nmessages {
                    let mut frame = vec![0; data_len];
                    stream.read_exact2(&mut frame).await?;
                    assert_eq!(&data[..], &frame[..]);
                }
            } else {
                let mut frame = vec![0; data_len];
                for _ in 0..nmessages {
                    stream.write_all2(&data).await?;
                    stream.read_exact2(&mut frame).await?;
                    assert_eq!(&data[..], &frame[..]);
                }
                stream.close2().await?;
            }

            tx.unbounded_send(nmessages).expect("unbounded_send");
            Ok::<(), ConnectionError>(())
        });
    }
    let n = rx.take(nstreams * nmessages).fold(0, |acc, n| future::ready(acc + n)).await;
    assert_eq!(nstreams, n);

    ctrl_client.close().await.expect("client close connection");
    // ctrl_server.close().await.expect("server close connection");
    stream_handle_server.await;
    loop_handle_client.await;
    loop_handle_server.await;
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
