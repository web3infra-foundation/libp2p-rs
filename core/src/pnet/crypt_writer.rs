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

use crate::transport::ConnectionInfo;
use crate::Multiaddr;
use async_trait::async_trait;
use futures::io;
use libp2prs_traits::{ReadEx, WriteEx};
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
    W: ReadEx + WriteEx + 'static,
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
impl<W> WriteEx for CryptWriter<W>
where
    W: ReadEx + WriteEx + 'static,
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

impl<W: WriteEx + fmt::Debug> fmt::Debug for CryptWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptWriter")
            .field("writer", &self.inner)
            .field("buf", &self.buf)
            .finish()
    }
}

#[async_trait]
impl<W> ReadEx for CryptWriter<W>
where
    W: ReadEx + WriteEx,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read2(buf).await
    }
}

impl<W: ConnectionInfo> ConnectionInfo for CryptWriter<W> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.inner.local_multiaddr()
    }
    fn remote_multiaddr(&self) -> Multiaddr {
        self.inner.remote_multiaddr()
    }
}
