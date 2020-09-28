use crate::{
    connection::{Id, StreamCommand},
    frame::{Frame, StreamID},
};
use async_trait::async_trait;
use bytes::{Buf, BufMut};
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2p_traits::{ReadEx, WriteEx};
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
    /// Closed (terminal state).
    Closed,
}

impl State {
    /// Can we receive messages over this stream?
    pub fn can_read(self) -> bool {
        if let State::RecvClosed | State::Closed = self {
            false
        } else {
            true
        }
    }

    /// Can we send messages over this stream?
    pub fn can_write(self) -> bool {
        if let State::SendClosed | State::Closed = self {
            false
        } else {
            true
        }
    }
}

#[derive(Clone)]
pub struct Stream {
    id: StreamID,
    conn_id: Id,
    read_buffer: bytes::BytesMut,
    sender: mpsc::Sender<StreamCommand>,
    receiver: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id.val())
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id.val())
    }
}

impl Stream {
    pub(crate) fn new(
        id: StreamID,
        conn_id: Id,
        sender: mpsc::Sender<StreamCommand>,
        receiver: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        Stream {
            id,
            conn_id,
            read_buffer: Default::default(),
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Get this stream's identifier.
    pub fn id(&self) -> u32 {
        self.id.val()
    }

    /// impl [`Clone`] trait
    pub fn clone(&self) -> Self {
        Stream {
            id: self.id,
            conn_id: self.conn_id,
            read_buffer: Default::default(),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
        }
    }

    // TODO: handle the case: buf capacity is not enough
    pub(crate) async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read_buffer.has_remaining() {
            let len = std::cmp::min(self.read_buffer.remaining(), buf.len());
            buf[..len].copy_from_slice(&self.read_buffer[..len]);
            self.read_buffer.advance(len);
            return Ok(len);
        }

        let mut receiver = self.receiver.lock().await;
        if let Some(data) = receiver.next().await {
            let dlen = data.len();
            let len = std::cmp::min(data.len(), buf.len());
            buf[..len].copy_from_slice(&data[..len]);

            if len < dlen {
                self.read_buffer.reserve(dlen - len);
                self.read_buffer.put(&data[len..dlen]);
            }
            return Ok(len);
        }

        Err(io::ErrorKind::UnexpectedEof.into())
    }

    pub(crate) async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (tx, rx) = oneshot::channel();
        let frame = Frame::message_frame(self.id, buf);
        let n = buf.len();
        log::trace!("{}/{}: write {} bytes", self.conn_id, self.id, n);

        let cmd = StreamCommand::SendFrame(frame, tx);
        self.sender
            .send(cmd)
            .await
            .map_err(|_| self.write_zero_err())?;

        rx.await.map_err(|_| self.closed_err())?;

        Ok(n)
    }

    async fn close(&mut self) -> io::Result<()> {
        // In order to support [`Clone`]
        // When reference count is 1, we can close stream entirely
        // Otherwise, just close sender channel
        let count = Arc::<Mutex<mpsc::Receiver<Vec<u8>>>>::strong_count(&self.receiver);
        if count == 1 {
            let frame = Frame::close_frame(self.id);
            let cmd = StreamCommand::CloseStream(frame);
            self.sender
                .send(cmd)
                .await
                .map_err(|_| self.write_zero_err())?;
        }

        // step3: close channel
        // self.sender.close().await.expect("send err");

        Ok(())
    }

    pub async fn reset(&mut self) -> io::Result<()> {
        // In order to support [`Clone`]
        // When reference count is 1, we can close stream entirely
        // Otherwise, just close sender channel
        let count = Arc::<Mutex<mpsc::Receiver<Vec<u8>>>>::strong_count(&self.receiver);
        if count == 1 {
            let frame = Frame::reset_frame(self.id);
            let cmd = StreamCommand::ResetStream(frame);
            self.sender
                .send(cmd)
                .await
                .map_err(|_| self.write_zero_err())?;

            self.sender
                .close()
                .await
                .map_err(|_| self.write_zero_err())?;
        }

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

#[async_trait]
impl ReadEx for Stream {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf).await
    }
}

#[async_trait]
impl WriteEx for Stream {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        Ok(())
    }

    async fn close2(&mut self) -> io::Result<()> {
        self.close().await
    }
}
