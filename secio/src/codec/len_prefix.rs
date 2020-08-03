use futures::prelude::*;
use std::{fmt, io};

/// `Stream` & `Sink` that reads and writes a length prefix in front of the actual data.
pub struct LengthPrefixSocket<T> {
    inner: T,
    max_frame_len: usize,
}

impl<T> fmt::Debug for LengthPrefixSocket<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("LengthPrefixSocket")
    }
}

impl<T> LengthPrefixSocket<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// create a new LengthPrefixSocket
    pub fn new(socket: T, max_len: usize) -> Self {
        Self {
            inner: socket,
            max_frame_len: max_len,
        }
    }

    pub(crate) async fn read_msg(&mut self) -> io::Result<Vec<u8>> {
        let mut len = [0; 4];
        let _ = self.inner.read_exact(&mut len).await?;

        let n = u32::from_be_bytes(len) as usize;
        if n > self.max_frame_len {
            let msg = format!(
                "data length {} exceeds allowed maximum {}",
                n, self.max_frame_len
            );
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, msg));
        }

        let mut v = vec![0; n];
        self.inner.read_exact(&mut v).await?;

        Ok(v)
    }

    pub(crate) async fn write_msg(&mut self, msg: Vec<u8>) -> io::Result<()> {
        self.inner
            .write_all(&(msg.len() as u32).to_be_bytes())
            .await?;
        self.inner.write_all(&msg).await
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().await
    }

    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.inner.close().await
    }
}
