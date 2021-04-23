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

use crate::{
    connection::{Id, StreamCommand},
    frame::{Frame, StreamID},
};
use bytes::{Buf, BufMut};
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures::task::{Context, Poll};
use futures::{channel::mpsc, AsyncRead, AsyncWrite, FutureExt, Sink, SinkExt};
use std::pin::Pin;
use std::sync::Arc;
use std::{fmt, io};

/// The state of a Yamux stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Open bidirectionally.
    Open,
    /// Open for incoming messages.
    SendClosed,
    /// Open for outgoing messages.
    RecvClosed,
}

impl State {
    /// Can we receive messages over this stream?
    pub fn can_read(self) -> bool {
        self != State::RecvClosed
    }

    /// Can we send messages over this stream?
    pub fn can_write(self) -> bool {
        self != State::SendClosed
    }
}

pub struct Stream {
    id: StreamID,
    conn_id: Id,
    read_buffer: bytes::BytesMut,
    sender: mpsc::Sender<StreamCommand>,
    receiver: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id.id())
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id.id())
    }
}

impl Clone for Stream {
    /// impl [`Clone`] trait
    fn clone(&self) -> Self {
        Stream {
            id: self.id,
            conn_id: self.conn_id,
            read_buffer: Default::default(),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
        }
    }
}

impl Stream {
    pub(crate) fn new(id: StreamID, conn_id: Id, sender: mpsc::Sender<StreamCommand>, receiver: mpsc::Receiver<Vec<u8>>) -> Self {
        Stream {
            id,
            conn_id,
            read_buffer: Default::default(),
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub fn val(&self) -> u32 {
        self.id.val()
    }

    /// Get this stream's identifier.
    pub fn id(&self) -> u32 {
        self.id.id()
    }

    /// reset stream, sender will be closed and state will turn to Closed
    /// If stream has reset, return ()
    pub async fn reset(&mut self) -> io::Result<()> {
        if self.sender.is_closed() {
            return Ok(());
        }

        let (tx, rx) = oneshot::channel();
        let frame = Frame::reset_frame(self.id);
        let cmd = StreamCommand::ResetStream(frame, tx);
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;
        rx.await.map_err(|_| self.closed_err())?;

        self.sender.close().await.map_err(|_| self.write_zero_err())?;

        Ok(())
    }

    /// connection is closed
    fn write_zero_err(&self) -> io::Error {
        let msg = format!("{}/{}: connection is closed", self.conn_id, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }

    /// stream is closed or reset
    fn closed_err(&self) -> io::Error {
        let msg = format!("{}/{}: stream is closed / reset", self.conn_id, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }
}

impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        if self.read_buffer.has_remaining() {
            let len = std::cmp::min(self.read_buffer.remaining(), buf.len());
            buf[..len].copy_from_slice(&self.read_buffer[..len]);
            self.read_buffer.advance(len);
            return Poll::Ready(Ok(len));
        }

        let this = self.get_mut();

        let mut receiver = futures::ready!(this.receiver.lock().poll_unpin(cx));

        let x = futures::Stream::poll_next(Pin::new(&mut *receiver), cx);
        if let Some(data) = futures::ready!(x) {
            let dlen = data.len();
            let len = std::cmp::min(data.len(), buf.len());
            buf[..len].copy_from_slice(&data[..len]);

            if len < dlen {
                this.read_buffer.reserve(dlen - len);
                this.read_buffer.put(&data[len..dlen]);
            }
            return Poll::Ready(Ok(len));
        }
        Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.sender.is_closed() {
            return Poll::Ready(Err(self.closed_err()));
        }

        futures::ready!(self.sender.poll_ready(cx).map_err(|_| self.write_zero_err())?);

        let frame = Frame::message_frame(self.id, buf);
        let n = buf.len();
        log::trace!("{}/{}: write {} bytes", self.conn_id, self.id, n);

        let cmd = StreamCommand::SendFrame(frame);
        self.sender.start_send(cmd).map_err(|_| self.write_zero_err())?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.sender).poll_flush(cx).map_err(|_| this.write_zero_err())
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.sender.is_closed() {
            return Poll::Ready(Ok(()));
        }

        futures::ready!(self.sender.poll_ready(cx).map_err(|_| self.write_zero_err())?);

        let frame = Frame::close_frame(self.id);
        let cmd = StreamCommand::CloseStream(frame);
        self.sender.start_send(cmd).map_err(|_| self.write_zero_err())?;

        let this = self.get_mut();
        Pin::new(&mut this.sender).poll_close(cx).map_err(|_| this.closed_err())
    }
}
