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

use async_trait::async_trait;
use fnv::FnvHashMap;
use futures::{channel::mpsc, prelude::*, task::Context, task::Poll};
use futures::{SinkExt, StreamExt};
use pin_project::pin_project;
use std::{collections::hash_map::Entry, fmt, io, num::NonZeroU64, pin::Pin};

use lazy_static::lazy_static;
use libp2prs_multiaddr::{protocol, protocol::Protocol, Multiaddr};
use parking_lot::Mutex;
use rw_stream_sink::RwStreamSink;

use crate::muxing::{IReadWrite, ReadWriteEx, StreamInfo};
use crate::transport::{ConnectionInfo, IListener, ITransport, TransportListener};
use crate::{transport::TransportError, Transport};
// use libp2prs_traits::SplitEx;
// use futures::io::{ReadHalf, WriteHalf};

lazy_static! {
    static ref HUB: Mutex<FnvHashMap<NonZeroU64, mpsc::Sender<Channel>>> = Mutex::new(FnvHashMap::default());
}

/// Transport that supports `/memory/N` multiaddresses.
///
/// MemoryTransport is mainly for test purpose, used to write test code for basic transport functionality.
#[derive(Debug, Clone, Default)]
pub struct MemoryTransport;

#[async_trait]
impl Transport for MemoryTransport {
    type Output = Channel;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        let port = if let Ok(port) = parse_memory_addr(&addr) {
            port
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let mut hub = (&*HUB).lock();

        let port = if let Some(port) = NonZeroU64::new(port) {
            port
        } else {
            loop {
                let port = match NonZeroU64::new(rand::random()) {
                    Some(p) => p,
                    None => continue,
                };
                if !hub.contains_key(&port) {
                    break port;
                }
            }
        };

        let (tx, rx) = mpsc::channel(2);
        match hub.entry(port) {
            Entry::Occupied(_) => return Err(TransportError::Unreachable),
            Entry::Vacant(e) => e.insert(tx),
        };

        let listener = Box::new(Listener {
            port,
            addr: Protocol::Memory(port.get()).into(),
            receiver: rx,
        });

        Ok(listener)
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let port = if let Ok(port) = parse_memory_addr(&addr) {
            if let Some(port) = NonZeroU64::new(port) {
                port
            } else {
                return Err(TransportError::Unreachable);
            }
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        // get a cloned sender, unlock the HUB asap
        let mut sender = {
            let hub = HUB.lock();
            if let Some(sender) = hub.get(&port) {
                sender.clone()
            } else {
                return Err(TransportError::Unreachable);
            }
        };

        let (a_tx, a_rx) = mpsc::channel(4096);
        let (b_tx, b_rx) = mpsc::channel(4096);

        let la = Multiaddr::empty();
        let ra = addr;

        let channel_to_send = Channel {
            io: RwStreamSink::new(Chan {
                incoming: a_rx,
                outgoing: b_tx,
            }),
            la: la.clone(),
            ra: ra.clone(),
        };
        let channel_to_return = Channel {
            io: RwStreamSink::new(Chan {
                incoming: b_rx,
                outgoing: a_tx,
            }),
            la: la.clone(),
            ra: ra.clone(),
        };
        sender.send(channel_to_send).await.map_err(|_| TransportError::Unreachable)?;
        Ok(channel_to_return)
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        Box::new(self.clone())
    }

    fn protocols(&self) -> Vec<u32> {
        vec![protocol::MEMORY]
    }
}

/// Listener for memory connections.
pub struct Listener {
    /// Port we're listening on.
    port: NonZeroU64,
    /// The address we are listening on.
    addr: Multiaddr,
    /// Receives incoming connections.
    receiver: mpsc::Receiver<Channel>,
}

#[async_trait]
impl TransportListener for Listener {
    type Output = Channel;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        self.receiver.next().await.ok_or(TransportError::Unreachable)
    }

    fn multi_addr(&self) -> Vec<Multiaddr> {
        vec![self.addr.clone()]
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let val_in = HUB.lock().remove(&self.port);
        debug_assert!(val_in.is_some());
    }
}

/// If the address is `/memory/n`, returns the value of `n`.
fn parse_memory_addr(a: &Multiaddr) -> Result<u64, ()> {
    let mut iter = a.iter();

    let port = if let Some(Protocol::Memory(port)) = iter.next() {
        port
    } else {
        return Err(());
    };

    if iter.next().is_some() {
        return Err(());
    }

    Ok(port)
}

/// A channel represents an established, in-memory, logical connection between two endpoints.
///
/// Implements `ReadEx` and `WriteEx`.
#[pin_project]
pub struct Channel {
    #[pin]
    io: RwStreamSink<Chan<Vec<u8>>>,
    la: Multiaddr,
    ra: Multiaddr,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel").field("la", &self.la).field("ra", &self.ra).finish()
    }
}

// Implements AsyncRead & AsyncWrite
impl AsyncRead for Channel {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.io.poll_read(cx, buf)
    }
}
impl AsyncWrite for Channel {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.io.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.io.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.io.poll_close(cx)
    }
}

/*
impl SplitEx for Channel {
    type Reader = ReadHalf<RwStreamSink<Chan>>;
    type Writer = WriteHalf<RwStreamSink<Chan>>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (r, w) = AsyncReadExt::split(self.io);
        (r, w)
    }
}
 */

/// A channel represents an established, in-memory, logical connection between two endpoints.
///
/// Implements `Sink` and `Stream`.
pub struct Chan<T = Vec<u8>> {
    incoming: mpsc::Receiver<T>,
    outgoing: mpsc::Sender<T>,
}

impl<T> Unpin for Chan<T> {}

impl<T> Stream for Chan<T> {
    type Item = Result<T, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match Stream::poll_next(Pin::new(&mut self.incoming), cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(Some(Err(io::ErrorKind::BrokenPipe.into()))),
            Poll::Ready(Some(v)) => Poll::Ready(Some(Ok(v))),
        }
    }
}

impl<T> Sink<T> for Chan<T> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.outgoing
            .poll_ready(cx)
            .map(|v| v.map_err(|_| io::ErrorKind::BrokenPipe.into()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.outgoing.start_send(item).map_err(|_| io::ErrorKind::BrokenPipe.into())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl ConnectionInfo for Channel {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl StreamInfo for Channel {
    fn id(&self) -> usize {
        0
    }
}

impl ReadWriteEx for Channel {
    fn box_clone(&self) -> IReadWrite {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2prs_traits::{ReadEx as _ReadEx, WriteEx as _WriteEx};

    #[test]
    fn parse_memory_addr_works() {
        assert_eq!(parse_memory_addr(&"/memory/5".parse().unwrap()), Ok(5));
        assert_eq!(parse_memory_addr(&"/tcp/150".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/memory/0".parse().unwrap()), Ok(0));
        assert_eq!(parse_memory_addr(&"/memory/5/tcp/150".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/tcp/150/memory/5".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/memory/1234567890".parse().unwrap()), Ok(1_234_567_890));
    }

    #[test]
    fn listening_twice() {
        let mut transport = MemoryTransport::default();
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        let _listener = transport.listen_on("/memory/1639174018481".parse().unwrap()).unwrap();
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_err());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_err());
        drop(_listener);
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
    }

    #[test]
    fn port_not_in_use() {
        futures::executor::block_on(async move {
            let mut transport = MemoryTransport::default();
            assert!(transport.dial("/memory/810172461024613".parse().unwrap()).await.is_err());
            let _listener = transport.listen_on("/memory/810172461024613".parse().unwrap()).unwrap();
            assert!(transport.dial("/memory/810172461024613".parse().unwrap()).await.is_ok());
        });
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        let msg = [1, 2, 3];

        // Setup listener.

        let rand_port = rand::random::<u64>().saturating_add(1);
        let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();
        let cloned_t1_addr = t1_addr.clone();

        let mut t1 = MemoryTransport::default();

        let listener = async move {
            let mut listener = t1.listen_on(t1_addr.clone()).unwrap();

            let mut socket = listener.accept().await.unwrap();

            let mut buf = [0; 3];
            socket.read_exact2(&mut buf).await.unwrap();

            assert_eq!(buf, msg);
        };

        // Setup dialer.

        let mut t2 = MemoryTransport::default();
        let dialer = async move {
            let mut socket = t2.dial(cloned_t1_addr).await.unwrap();
            socket.write_all2(&msg).await.unwrap();
        };

        // Wait for both to finish.

        futures::executor::block_on(futures::future::join(listener, dialer));
    }
}
