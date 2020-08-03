use futures::prelude::*;
use std::{fmt, io};
use futures::{AsyncReadExt, AsyncWriteExt};
use async_trait::async_trait;
use libp2p_traits::{Read, Write};


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
}

#[async_trait]
impl<T> Read for LengthPrefixSocket<T>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    async fn read(&mut self) -> io::Result<Vec<u8>> {
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
}

#[async_trait]
impl<T> Write for LengthPrefixSocket<T>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    async fn write(&mut self, buf: &Vec<u8>) -> io::Result<()> {
        self.inner
            .write_all(&(buf.len() as u32).to_be_bytes())
            .await?;
        self.inner.write_all(&buf).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().await
    }

    async fn close(&mut self) -> io::Result<()> {
        self.inner.close().await
    }
}
