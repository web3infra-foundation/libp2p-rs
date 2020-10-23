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

use bytes::{BufMut, BytesMut};
use libp2prs_traits::{ReadEx, WriteEx};
use std::{fmt, io, time::Duration};
use async_std::task;

/// Use `ReadEx` & `WriteEx` to read and write a length prefix in front of the actual data.
pub struct LengthPrefixSocket<T> {
    inner: T,
    max_frame_len: usize,
}

impl<T> fmt::Debug for LengthPrefixSocket<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("LengthPrefixSocket")
    }
}

impl<T> LengthPrefixSocket<T> {
    /// create a new LengthPrefixSocket
    pub fn new(socket: T, max_len: usize) -> Self {
        Self {
            inner: socket,
            max_frame_len: max_len,
        }
    }
}

impl<T> LengthPrefixSocket<T>
where
    T: ReadEx + 'static,
{
    /// convenient method for reading a whole frame
    #[allow(clippy::let_unit_value)]
    pub async fn recv_frame(&mut self) -> io::Result<Vec<u8>> {
        let mut len = [0; 4];
        let _ = self.inner.read_exact2(&mut len).await?;

        let n = u32::from_be_bytes(len) as usize;
        if n > self.max_frame_len {
            let msg = format!("data length {} exceeds allowed maximum {}", n, self.max_frame_len);
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, msg));
        }
        task::sleep(Duration::from_secs(3)).await;

        let mut frame = vec![0; n];
        self.inner.read_exact2(&mut frame).await?;

        Ok(frame)
    }
}

impl<T> LengthPrefixSocket<T>
where
    T: WriteEx + 'static,
{
    /// sending a length delimited frame
    pub async fn send_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        let mut buf = BytesMut::with_capacity(frame.len() + 4);

        buf.put_u32(frame.len() as u32);
        buf.put(frame);
        self.inner.write_all2(&buf).await
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush2().await
    }
    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.inner.close2().await
    }
}
