use crate::{
    connection::{Id, StreamCommand},
    frame::{Frame, StreamID},
};
use async_trait::async_trait;
use bytes::{Buf, BufMut};
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2p_traits::{Read2, Write2};
use std::{fmt, io};
use futures::lock::Mutex;
use std::sync::Arc;

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
        let frame = Frame::message_frame(self.id, buf);
        let n = buf.len();
        log::trace!("{}/{}: write {} bytes", self.conn_id, self.id, n);

        let cmd = StreamCommand::SendFrame(frame);
        self.sender
            .send(cmd)
            .await
            .map_err(|_| self.write_zero_err())?;

        Ok(n)
    }

    async fn close(&mut self) -> io::Result<()> {
        // in order to support [`Clone`]
        // when reference count is 1, we can close stream entirely
        // otherwise, just close sender channel
        let count = Arc::<Mutex<mpsc::Receiver<Vec<u8>>>>::strong_count(&self.receiver);
        if count == 1 {
            // step1: send close frame
            let frame = Frame::close_frame(self.id);
            let cmd = StreamCommand::SendFrame(frame);
            self.sender
                .send(cmd)
                .await
                .map_err(|_| self.write_zero_err())?;

            // step2: notify connection to remove itself
            let cmd = StreamCommand::CloseStream(self.id);
            self.sender
                .send(cmd)
                .await
                .map_err(|_| self.write_zero_err())?;
        }

        // step3: close channel
        self.sender.close().await.expect("send err");

        Ok(())
    }

    fn write_zero_err(&self) -> io::Error {
        let msg = format!("{}/{}: connection is closed", self.conn_id, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }
}

#[async_trait]
impl Read2 for Stream {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf).await
    }
}

#[async_trait]
impl Write2 for Stream {
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
