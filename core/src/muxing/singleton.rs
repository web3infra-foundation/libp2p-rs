use crate::{muxing::{StreamMuxer, StreamMuxerEvent}};

use futures::prelude::*;
use parking_lot::Mutex;
use std::{io, pin::Pin, sync::atomic::{AtomicBool, Ordering}, task::Context, task::Poll};

/// Implementation of `StreamMuxer` that allows only one substream on top of a connection,
/// yielding the connection itself.
///
/// Applying this muxer on a connection doesn't read or write any data on the connection itself.
/// Most notably, no protocol is negotiated.
pub struct SingletonMuxer<TSocket> {
    /// The inner connection.
    inner: Mutex<TSocket>,
    /// If true, a substream has been produced and any further attempt should fail.
    substream_extracted: AtomicBool,
    /// Our local endpoint. Always the same value as was passed to `new`.
    endpoint: Endpoint,
}

impl<TSocket> SingletonMuxer<TSocket> {
    /// Creates a new `SingletonMuxer`.
    ///
    /// If `endpoint` is `Dialer`, then only one outbound substream will be permitted.
    /// If `endpoint` is `Listener`, then only one inbound substream will be permitted.
    pub fn new(inner: TSocket, endpoint: Endpoint) -> Self {
        SingletonMuxer {
            inner: Mutex::new(inner),
            substream_extracted: AtomicBool::new(false),
            endpoint,
        }
    }
}

/// Substream of the `SingletonMuxer`.
pub struct Substream {}
/// Outbound substream attempt of the `SingletonMuxer`.
pub struct OutboundSubstream {}

impl<TSocket> StreamMuxer for SingletonMuxer<TSocket>
where
    TSocket: AsyncRead + AsyncWrite + Unpin,
{
    type Substream = Substream;
    type OutboundSubstream = OutboundSubstream;
    type Error = io::Error;

    fn poll_event(&self, _: &mut Context) -> Poll<Result<StreamMuxerEvent<Self::Substream>, io::Error>> {
        match self.endpoint {
            Endpoint::Dialer => return Poll::Pending,
            Endpoint::Listener => {}
        }

        if !self.substream_extracted.swap(true, Ordering::Relaxed) {
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(Substream {})))
        } else {
            Poll::Pending
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        OutboundSubstream {}
    }

    fn poll_outbound(&self, _: &mut Context, _: &mut Self::OutboundSubstream) -> Poll<Result<Self::Substream, io::Error>> {
        match self.endpoint {
            Endpoint::Listener => return Poll::Pending,
            Endpoint::Dialer => {}
        }

        if !self.substream_extracted.swap(true, Ordering::Relaxed) {
            Poll::Ready(Ok(Substream {}))
        } else {
            Poll::Pending
        }
    }

    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
    }

    fn read_substream(&self, cx: &mut Context, _: &mut Self::Substream, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut *self.inner.lock()), cx, buf)
    }

    fn write_substream(&self, cx: &mut Context, _: &mut Self::Substream, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut *self.inner.lock()), cx, buf)
    }

    fn flush_substream(&self, cx: &mut Context, _: &mut Self::Substream) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut *self.inner.lock()), cx)
    }

    fn shutdown_substream(&self, cx: &mut Context, _: &mut Self::Substream) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut *self.inner.lock()), cx)
    }

    fn destroy_substream(&self, _: Self::Substream) {
    }

    fn close(&self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // The `StreamMuxer` trait requires that `close()` implies `flush_all()`.
        self.flush_all(cx)
    }

    fn flush_all(&self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut *self.inner.lock()), cx)
    }
}
