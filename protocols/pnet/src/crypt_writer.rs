use async_trait::async_trait;
use futures::io;
use libp2p_traits::{Read2, Write2};
use log::trace;
use salsa20::{stream_cipher::SyncStreamCipher, XSalsa20};
use std::fmt;

/// A writer that encrypts and forwards to an inner writer
pub struct CryptWriter<W> {
    inner: W,
    buf: Vec<u8>,
    cipher: XSalsa20,
}

impl<W> CryptWriter<W>
where
    W: Read2 + Write2 + Send + 'static,
{
    /// Creates a new `CryptWriter` with the specified buffer capacity.
    pub fn with_capacity(capacity: usize, inner: W, cipher: XSalsa20) -> CryptWriter<W> {
        CryptWriter {
            inner,
            buf: Vec::with_capacity(capacity),
            cipher,
        }
    }
}

#[async_trait]
impl<W> Write2 for CryptWriter<W>
where
    W: Read2 + Write2 + Send + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.append(&mut buf.to_vec());
        trace!("write bytes: {:?} ", self.buf);
        self.cipher.apply_keystream(&mut self.buf[0..buf.len()]);
        trace!("crypted bytes: {:?}", self.buf);
        let size = self.inner.write2(&self.buf[..]).await?;
        self.buf.drain(..);
        Ok(size)
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.inner.flush2().await
    }
    async fn close2(&mut self) -> io::Result<()> {
        self.inner.close2().await
    }
}

impl<W: Write2 + fmt::Debug> fmt::Debug for CryptWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptWriter")
            .field("writer", &self.inner)
            .field("buf", &self.buf)
            .finish()
    }
}

#[async_trait]
impl<W> Read2 for CryptWriter<W>
where
    W: Read2 + Write2 + Send,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read2(buf).await
    }
}
