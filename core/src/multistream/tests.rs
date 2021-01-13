// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Contains the unit tests of the library.

#![cfg(test)]

use super::negotiator::Negotiator;
use super::{NegotiationError, ReadEx, Version, WriteEx};

use async_std::net::{TcpListener, TcpStream};
use async_trait::async_trait;
use bytes::Bytes;
use futures::channel::mpsc;
use futures::prelude::*;
use std::io;

#[derive(Debug)]
pub struct Memory<T> {
    tx: mpsc::Sender<T>,
    rx: mpsc::Receiver<T>,

    recv_drian: Option<T>,
}

impl Memory<Bytes> {
    pub fn pair() -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel(1);
        let (tx2, rx2) = mpsc::channel(1);
        (
            Memory {
                tx: tx1,
                rx: rx2,
                recv_drian: None,
            },
            Memory {
                tx: tx2,
                rx: rx1,
                recv_drian: None,
            },
        )
    }

    fn drain(&mut self, buf: &mut [u8]) -> Option<usize> {
        if let Some(b) = &mut self.recv_drian {
            // calculate number of bytes that we can copy
            let n = ::std::cmp::min(buf.len(), b.len());
            if n == 0 {
                return None;
            }
            buf[..n].copy_from_slice(b[..n].as_ref());
            *b = b.split_off(n);
            Some(n)
        } else {
            None
        }
    }
}

#[async_trait]
impl ReadEx for Memory<Bytes> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(n) = self.drain(buf) {
            return Ok(n);
        }
        let b = self.rx.next().await.expect("recv next");
        self.recv_drian.replace(b);
        Ok(self.drain(buf).expect("must be Some(n)"))
    }
}

#[async_trait]
impl WriteEx for Memory<Bytes> {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        log::debug!("write data: {:?}", buf);
        self.tx
            .send(Bytes::copy_from_slice(buf))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(buf.len())
    }

    async fn flush2(&mut self) -> io::Result<()> {
        Ok(())
    }

    async fn close2(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn select_proto_basic() {
    async fn run(_version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec!["/proto11", "/proto2"];
            let neg = Negotiator::new_with_protocols(protos);
            let (proto, mut io) = neg.negotiate(connec).await.expect("negotiate");
            assert_eq!(proto, "/proto2");

            let mut out = vec![0; 32];
            let n = io.read(&mut out).await.unwrap();
            out.truncate(n);
            assert_eq!(out, b"ping");

            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec!["/proto31", "/proto2"];
            let neg = Negotiator::new_with_protocols(protos);
            let (proto, mut io) = neg.select_one(connec).await.expect("select_one");
            assert_eq!(proto, "/proto2");

            io.write_all(b"ping").await.unwrap();
            io.flush().await.unwrap();

            let mut out = vec![0; 32];
            let n = io.read(&mut out).await.unwrap();
            out.truncate(n);
            assert_eq!(out, b"pong");
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
}

#[test]
fn no_protocol_found() {
    async fn run(_version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let neg = Negotiator::new_with_protocols(protos);

            // We don't explicitly check for `Failed` because the client might close the connection when it
            // realizes that we have no protocol in common.
            assert!(neg.negotiate(connec).await.is_err());
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto4"];
            let neg = Negotiator::new_with_protocols(protos);
            match neg.select_one(connec).await {
                Err(NegotiationError::Failed(_)) => {}
                Ok(_) => {}
                Err(_) => panic!(),
            }
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
}

#[test]
fn select_proto_serial() {
    async fn run(_version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let neg = Negotiator::new_with_protocols(protos);
            let (proto, _) = neg.negotiate(connec).await.expect("negotiate");
            assert_eq!(proto, b"/proto2");
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto2"];
            let neg = Negotiator::new_with_protocols(protos);
            let (proto, _) = neg.select_one(connec).await.expect("select_one");
            assert_eq!(proto, b"/proto2");
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
}
