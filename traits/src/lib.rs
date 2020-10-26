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

mod copy;
pub mod ext;

use std::io;
use std::io::ErrorKind;

use async_trait::async_trait;
use futures::io::{ReadHalf, WriteHalf};
use futures::prelude::*;
use futures::{AsyncReadExt, AsyncWriteExt};

pub use copy::copy;
pub use ext::ReadExt2;

/// Read Trait for async/await
///
#[async_trait]
pub trait ReadEx: Send {
    /// Reads some bytes from the byte stream.
    ///
    /// On success, returns the total number of bytes read.
    ///
    /// If the return value is `Ok(n)`, then it must be guaranteed that
    /// `0 <= n <= buf.len()`. A nonzero `n` value indicates that the buffer has been
    /// filled with `n` bytes of data. If `n` is `0`, then it can indicate one of two
    /// scenarios:
    ///
    /// 1. This reader has reached its "end of file" and will likely no longer be able to
    ///    produce bytes. Note that this does not mean that the reader will always no
    ///    longer be able to produce bytes.
    /// 2. The buffer specified was 0 bytes in length.
    ///
    /// Attempt to read bytes from underlying stream object.
    ///
    /// On success, returns `Ok(n)`.
    /// Otherwise, returns `Err(io:Error)`
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error>;

    /// Reads the exact number of bytes requested.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`.
    async fn read_exact2<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<(), io::Error> {
        let mut buf_piece = buf;
        while !buf_piece.is_empty() {
            let n = self.read2(buf_piece).await?;
            if n == 0 {
                return Err(ErrorKind::UnexpectedEof.into());
            }

            let (_, rest) = buf_piece.split_at_mut(n);
            buf_piece = rest;
        }
        Ok(())
    }

    /// Reads a fixed-length integer from the underlying IO.
    ///
    /// On success, return `Ok(n)`.
    /// Otherwise, returns `Err(io:Error)`.
    async fn read_fixed_u32(&mut self) -> Result<usize, io::Error> {
        let mut len = [0; 4];
        self.read_exact2(&mut len).await?;
        let n = u32::from_be_bytes(len) as usize;

        Ok(n)
    }

    /// Reads a variable-length integer from the underlying IO.
    ///
    /// As a special exception, if the `IO` is empty and EOFs right at the beginning, then we
    /// return `Ok(0)`.
    ///
    /// On success, return `Ok(n)`.
    /// Otherwise, returns `Err(io:Error)`.
    ///
    /// > **Note**: This function reads bytes one by one from the underlying IO. It is therefore encouraged
    /// >           to use some sort of buffering mechanism.
    async fn read_varint(&mut self) -> Result<usize, io::Error> {
        let mut buffer = unsigned_varint::encode::usize_buffer();
        let mut buffer_len = 0;

        loop {
            match self.read2(&mut buffer[buffer_len..=buffer_len]).await? {
                0 => {
                    // Reaching EOF before finishing to read the length is an error, unless the EOF is
                    // at the very beginning of the substream, in which case we assume that the data is
                    // empty.
                    if buffer_len == 0 {
                        return Ok(0);
                    } else {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                }
                n => debug_assert_eq!(n, 1),
            }

            buffer_len += 1;

            match unsigned_varint::decode::usize(&buffer[..buffer_len]) {
                Ok((len, _)) => return Ok(len),
                Err(unsigned_varint::decode::Error::Overflow) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "overflow in variable-length integer"));
                }
                // TODO: why do we have a `__Nonexhaustive` variant in the error? I don't know how to process it
                // Err(unsigned_varint::decode::Error::Insufficient) => {}
                Err(_) => {}
            }
        }
    }

    /// Reads a fixed length-prefixed message from the underlying IO.
    ///
    /// The `max_size` parameter is the maximum size in bytes of the message that we accept. This is
    /// necessary in order to avoid DoS attacks where the remote sends us a message of several
    /// gigabytes.
    ///
    /// > **Note**: Assumes that a fixed-length prefix indicates the length of the message. This is
    /// >           compatible with what `write_one_fixed` does.
    async fn read_one_fixed(&mut self, max_size: usize) -> Result<Vec<u8>, io::Error> {
        let len = self.read_fixed_u32().await?;
        if len > max_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Received data size over maximum frame length: {}>{}", len, max_size),
            ));
        }

        let mut buf = vec![0; len];
        self.read_exact2(&mut buf).await?;
        Ok(buf)
    }

    /// Reads a variable length-prefixed message from the underlying IO.
    ///
    /// The `max_size` parameter is the maximum size in bytes of the message that we accept. This is
    /// necessary in order to avoid DoS attacks where the remote sends us a message of several
    /// gigabytes.
    ///
    /// On success, returns `Ok(Vec<u8>)`.
    /// Otherwise, returns `Err(io:Error)`.
    ///
    /// > **Note**: Assumes that a variable-length prefix indicates the length of the message. This is
    /// >           compatible with what `write_one` does.
    async fn read_one(&mut self, max_size: usize) -> Result<Vec<u8>, io::Error> {
        let len = self.read_varint().await?;
        if len > max_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Received data size over maximum frame length: {}>{}", len, max_size),
            ));
        }

        let mut buf = vec![0; len];
        self.read_exact2(&mut buf).await?;
        Ok(buf)
    }
}

/// Write Trait for async/await
///
#[async_trait]
pub trait WriteEx: Send {
    /// Attempt to write bytes from `buf` into the object.
    ///
    /// On success, returns `Ok(num_bytes_written)`.
    /// Otherwise, returns `Err(io:Error)`
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error>;
    /// Attempt to write the entire contents of data into object.
    ///
    /// The operation will not complete until all the data has been written.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    async fn write_all2(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        let mut buf_piece = buf;
        while !buf_piece.is_empty() {
            let n = self.write2(buf_piece).await?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }

            let (_, rest) = buf_piece.split_at(n);
            buf_piece = rest;
        }
        Ok(())
    }

    /// Writes a variable-length integer to the underlying IO.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    ///
    /// > **Note**: Does **NOT** flush the IO.
    async fn write_varint(&mut self, len: usize) -> Result<(), io::Error> {
        let mut len_data = unsigned_varint::encode::usize_buffer();
        let encoded_len = unsigned_varint::encode::usize(len, &mut len_data).len();
        self.write_all2(&len_data[..encoded_len]).await?;
        Ok(())
    }

    /// Writes a fixed-length integer to the underlying IO.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    ///
    /// > **Note**: Does **NOT** flush the IO.
    async fn write_fixed_u32(&mut self, len: usize) -> Result<(), io::Error> {
        self.write_all2((len as u32).to_be_bytes().as_ref()).await?;
        Ok(())
    }

    /// Send a fixed length message to the underlying IO, then flushes the writing side.
    ///
    /// > **Note**: Prepends a fixed-length prefix indicate the length of the message. This is
    /// >           compatible with what `read_one_fixed` expects.
    async fn write_one_fixed(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        self.write_fixed_u32(buf.len()).await?;
        self.write_all2(buf).await?;
        self.flush2().await?;
        Ok(())
    }

    /// Send a variable length message to the underlying IO, then flushes the writing side.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    ///
    /// > **Note**: Prepends a variable-length prefix indicate the length of the message. This is
    /// >           compatible with what `read_one` expects.
    async fn write_one(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        self.write_varint(buf.len()).await?;
        self.write_all2(buf).await?;
        self.flush2().await?;
        Ok(())
    }

    /// Attempt to flush the object, ensuring that any buffered data reach
    /// their destination.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    async fn flush2(&mut self) -> Result<(), io::Error>;

    /// Attempt to close the object.
    ///
    /// On success, returns `Ok(())`.
    /// Otherwise, returns `Err(io:Error)`
    async fn close2(&mut self) -> Result<(), io::Error>;
}

#[async_trait]
impl<T: AsyncRead + Unpin + Send> ReadEx for T {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let n = AsyncReadExt::read(self, buf).await?;
        Ok(n)
    }
}

#[async_trait]
impl<T: AsyncWrite + Unpin + Send> WriteEx for T {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        AsyncWriteExt::write(self, buf).await
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        AsyncWriteExt::flush(self).await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        AsyncWriteExt::close(self).await
    }
}

///
/// SplitEx Trait for read and write separation
///
pub trait SplitEx {
    /// read half
    type Reader: ReadEx + Unpin;
    /// write half
    type Writer: WriteEx + Unpin;

    /// split Self to independent reader and writer
    fn split(self) -> (Self::Reader, Self::Writer);
}

// a common way to support SplitEx for T, requires T: AsyncRead+AsyncWrite
impl<T: AsyncRead + AsyncWrite + Send + Unpin> SplitEx for T {
    type Reader = ReadHalf<T>;
    type Writer = WriteHalf<T>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        futures::AsyncReadExt::split(self)
    }
}

// the other way around to implement SplitEx for T, requires T supports Clone
// which async-std::TcpStream does
/*
impl<T: ReadEx + WriteEx + Unpin + Clone> SplitEx for T {
    type Reader = T;
    type Writer = T;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let r = self.clone();
        let w = self;
        (r, w)
    }
}
*/

/// SplittableReadWrite trait for simplifying generic type constraints
pub trait SplittableReadWrite: ReadEx + WriteEx + SplitEx + Unpin + 'static {}

impl<T: ReadEx + WriteEx + SplitEx + Unpin + 'static> SplittableReadWrite for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::{self, AsyncReadExt, Cursor};

    struct Test(Cursor<Vec<u8>>);

    #[async_trait]
    impl ReadEx for Test {
        async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
            self.0.read(buf).await
        }
    }

    #[async_trait]
    impl WriteEx for Test {
        async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
            self.0.write(buf).await
        }

        async fn flush2(&mut self) -> Result<(), io::Error> {
            self.0.flush().await
        }

        async fn close2(&mut self) -> Result<(), io::Error> {
            self.0.close().await
        }
    }

    /// Read Vec<u8>
    #[test]
    fn test_read() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(vec![1, 2, 3, 4]));
            let mut output = [0u8; 3];
            let bytes = reader.read2(&mut output[..]).await.unwrap();

            assert_eq!(bytes, 3);
            assert_eq!(output, [1, 2, 3]);
        });
    }

    // Read string
    #[test]
    fn test_read_string() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(b"hello world".to_vec()));
            let mut output = [0u8; 3];
            let bytes = reader.read2(&mut output[..]).await.unwrap();

            assert_eq!(bytes, 3);
            assert_eq!(output, [104, 101, 108]);
        });
    }

    #[test]
    fn test_read_exact() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(vec![1, 2, 3, 4]));
            let mut output = [0u8; 3];
            let _bytes = reader.read_exact2(&mut output[..]).await;

            assert_eq!(output, [1, 2, 3]);
        });
    }

    #[test]
    fn test_read_fixed_u32() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(b"hello world".to_vec()));
            let size = reader.read_fixed_u32().await.unwrap();

            assert_eq!(size, 1751477356);
        });
    }

    #[test]
    fn test_read_varint() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(vec![1, 2, 3, 4, 5, 6]));
            let size = reader.read_varint().await.unwrap();

            assert_eq!(size, 1);
        });
    }

    #[test]
    fn test_read_one() {
        async_std::task::block_on(async {
            let mut reader = Test(Cursor::new(vec![11, 104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100]));
            let output = match reader.read_one(11).await {
                Ok(v) => v,
                _ => Vec::new(),
            };

            assert_eq!(output, b"hello world");
        });
    }

    #[test]
    fn test_write() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(vec![0u8; 5]));
            let size = writer.write2(&[1, 2, 3, 4]).await.unwrap();

            assert_eq!(size, 4);
            assert_eq!(writer.0.get_mut(), &[1, 2, 3, 4, 0])
        });
    }

    #[test]
    fn test_write_all2() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(vec![0u8; 4]));
            let output = vec![1, 2, 3, 4, 5];
            let _bytes = writer.write_all2(&output[..]).await.unwrap();

            assert_eq!(writer.0.get_mut(), &[1, 2, 3, 4, 5]);
        });
    }

    #[test]
    fn test_write_fixed_u32() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(b"hello world".to_vec()));
            let _result = writer.write_fixed_u32(1751477356).await.unwrap();

            // Binary value of `hell` is 17751477356, if write successfully, current
            // pointer ought to stay on 4
            assert_eq!(writer.0.position(), 4);
        });
    }

    #[test]
    fn test_write_varint() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(vec![2, 2, 3, 4, 5, 6]));
            let _result = writer.write_varint(1).await.unwrap();

            assert_eq!(writer.0.position(), 1);
        });
    }

    #[test]
    fn test_write_one() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(vec![0u8; 0]));
            let _result = writer.write_one("hello world".as_ref()).await;
            assert_eq!(writer.0.get_mut(), &[11, 104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100]);
        });
    }
}
