// Copyright 2019 Parity Technologies (UK) Ltd.
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
use futures::io::Error;
use libp2prs_traits::ReadEx;
use salsa20::cipher::SyncStreamCipher;
use salsa20::XSalsa20;

pub struct CryptReader<R> {
    inner: R,
    cipher: XSalsa20,
}

impl<R> CryptReader<R>
where
    R: ReadEx + 'static,
{
    /// Creates a new `CryptReader`.
    pub fn new(inner: R, cipher: XSalsa20) -> Self {
        CryptReader { inner, cipher }
    }
}

#[async_trait]
impl<R> ReadEx for CryptReader<R>
where
    R: ReadEx + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let size = self.inner.read2(buf).await?;
        log::trace!("read {} bytes", size);
        self.cipher.apply_keystream(&mut buf[..size]);
        log::trace!("decrypted {} bytes", size);
        Ok(size)
    }
}
