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

use log::debug;

use std::io;

use async_trait::async_trait;
use libp2prs_traits::{ReadEx, Split, WriteEx};

/// SecureStreamReader
pub struct SecureStreamReader<R> {
    socket: R,
    max_frame_len: usize,
    /// recv buffer
    /// internal buffer for 'message too big'
    ///
    /// when the input buffer is not big enough to hold the entire
    /// frame from the underlying Framed<>, the frame will be filled
    /// into this buffer so that multiple following 'read' will eventually
    /// get the message correctly
    recv_buf: Vec<u8>,
}

impl<R> SecureStreamReader<R>
where
    R: ReadEx + 'static,
{
    fn new(socket: R, max_frame_len: usize) -> Self {
        SecureStreamReader {
            socket,
            max_frame_len,
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
impl<R> ReadEx for SecureStreamReader<R>
where
    R: ReadEx + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // when there is somthing in recv_buffer
        let copied = self.drain(buf);
        if copied > 0 {
            debug!("drain recv buffer data size: {:?}", copied);
            return Ok(copied);
        }

        let t = self.socket.read_one_fixed(self.max_frame_len).await?;
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

pub struct SecureStreamWriter<W> {
    socket: W,
}

impl<W> SecureStreamWriter<W>
where
    W: WriteEx + 'static,
{
    fn new(socket: W) -> Self {
        SecureStreamWriter { socket }
    }
}

#[async_trait]
impl<R> WriteEx for SecureStreamWriter<R>
where
    R: WriteEx + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        debug!("start sending plain data: {:?}", buf);

        self.socket.write_one_fixed(buf).await?;
        Ok(buf.len())
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.socket.flush2().await
    }
    async fn close2(&mut self) -> io::Result<()> {
        self.socket.close2().await
    }
}

/// Encrypted stream
pub struct SecureStream<R, W> {
    reader: SecureStreamReader<R>,
    writer: SecureStreamWriter<W>,
}

impl<R, W> SecureStream<R, W>
where
    R: ReadEx + 'static,
    W: WriteEx + 'static,
{
    /// New a secure stream
    pub(crate) fn new(reader: R, writer: W, max_frame_len: usize) -> Self {
        SecureStream {
            reader: SecureStreamReader::new(reader, max_frame_len),
            writer: SecureStreamWriter::new(writer),
        }
    }
}

#[async_trait]
impl<R, W> ReadEx for SecureStream<R, W>
where
    R: ReadEx + 'static,
    W: WriteEx + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.reader.read2(buf).await
    }
}

#[async_trait]
impl<R, W> WriteEx for SecureStream<R, W>
where
    R: ReadEx + 'static,
    W: WriteEx + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.writer.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        self.writer.flush2().await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        self.writer.close2().await
    }
}

impl<R, W> Split for SecureStream<R, W>
where
    R: ReadEx + Unpin + 'static,
    W: WriteEx + Unpin + 'static,
{
    type Reader = SecureStreamReader<R>;
    type Writer = SecureStreamWriter<W>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (self.reader, self.writer)
    }
}
