use log::debug;

use std::io;

use crate::codec::len_prefix::LengthPrefixSocket;

use async_trait::async_trait;
use libp2p_traits::{Read2, Write2};

/// Encrypted stream
pub struct SecureStream<T> {
    socket: LengthPrefixSocket<T>,
    /// recv buffer
    /// internal buffer for 'message too big'
    ///
    /// when the input buffer is not big enough to hold the entire
    /// frame from the underlying Framed<>, the frame will be filled
    /// into this buffer so that multiple following 'read' will eventually
    /// get the message correctly
    recv_buf: Vec<u8>,
}

impl<T> SecureStream<T>
where
    T: Read2 + Write2 + Send + 'static,
{
    /// New a secure stream
    pub(crate) fn new(socket: LengthPrefixSocket<T>) -> Self {
        SecureStream {
            socket,
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
impl<T> Read2 for SecureStream<T>
where
    T: Read2 + Write2 + Send + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // when there is somthing in recv_buffer
        let copied = self.drain(buf);
        if copied > 0 {
            debug!("drain recv buffer data size: {:?}", copied);
            return Ok(copied);
        }

        let t = self.socket.recv_frame().await?;
        debug!("receive data size: {:?}", t.len());

        // when input buffer is big enough
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

#[async_trait]
impl<T> Write2 for SecureStream<T>
where
    T: Read2 + Write2 + Send + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        debug!("start sending plain data: {:?}", buf);

        self.socket.send_frame(buf).await?;
        Ok(buf.len())
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.socket.flush().await
    }
    async fn close2(&mut self) -> io::Result<()> {
        self.socket.close().await
    }
}
