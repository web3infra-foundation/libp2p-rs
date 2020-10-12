mod copy;
pub mod ext;

use async_trait::async_trait;
use futures::prelude::*;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::io;
use std::io::ErrorKind;

pub use copy::copy;
pub use ext::ReadExt2;

/// Read Trait for async/wait
///
#[async_trait]
pub trait ReadEx {
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
    /// # Examples
    ///
    /// ```no_run
    /// use blocking::Unblock;
    /// use futures_lite::*;
    /// use std::fs::File;
    ///
    /// # future::block_on(async {
    /// let mut file = Unblock::new(File::open("a.txt")?);
    ///
    /// let mut buf = vec![0; 1024];
    /// let n = file.read(&mut buf).await?;
    /// # std::io::Result::Ok(()) });
    /// ```
    /// Attempt to read bytes from underlying stream object.
    ///
    /// On success, returns `Ok(Vec<u8>)`.
    /// Otherwise, returns io::Error
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error>;

    /// Reads the exact number of bytes requested.
    ///
    /// On success, returns the total number of bytes read.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # future::block_on(async {
    /// let mut reader = io::Cursor::new(&b"hello");
    /// let mut contents = vec![0; 3];
    ///
    /// reader.read_exact(&mut contents).await?;
    /// assert_eq!(contents, b"hel");
    /// # std::io::Result::Ok(()) });
    /// ```
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
    /// > **Note**: This function reads bytes one by one from the underlying IO. It is therefore encouraged
    /// >           to use some sort of buffering mechanism.
    async fn read_varint(&mut self) -> Result<usize, io::Error> {
        let mut buffer = unsigned_varint::encode::usize_buffer();
        let mut buffer_len = 0;

        loop {
            match self.read2(&mut buffer[buffer_len..buffer_len + 1]).await? {
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

    /// Reads a length-prefixed message from the underlying IO.
    ///
    /// The `max_size` parameter is the maximum size in bytes of the message that we accept. This is
    /// necessary in order to avoid DoS attacks where the remote sends us a message of several
    /// gigabytes.
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

/// Write Trait for async/wait
///
#[async_trait]
pub trait WriteEx {
    /// Attempt to write bytes from `buf` into the object.
    ///
    /// On success, returns `Ok(num_bytes_written)`.
    /// Otherwise, returns io::Error
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error>;
    /// Attempt to write the entire contents of data into object.
    ///
    /// The operation will not complete until all the data has been written.
    ///
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
    /// > **Note**: Does **NOT** flush the IO.
    async fn write_varint(&mut self, len: usize) -> Result<(), io::Error> {
        let mut len_data = unsigned_varint::encode::usize_buffer();
        let encoded_len = unsigned_varint::encode::usize(len, &mut len_data).len();
        self.write_all2(&len_data[..encoded_len]).await?;
        Ok(())
    }

    /// Writes a fixed-length integer to the underlying IO.
    ///
    /// > **Note**: Does **NOT** flush the IO.
    async fn write_fixed_u32(&mut self, len: usize) -> Result<(), io::Error> {
        self.write_all2((len as u32).to_be_bytes().as_ref()).await?;
        Ok(())
    }

    /// Send a message to the underlying IO, then flushes the writing side.
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
    async fn flush2(&mut self) -> Result<(), io::Error>;

    /// Attempt to close the object.
    ///
    /// On success, returns `Poll::Ready(Ok(()))`.
    ///
    /// If closing cannot immediately complete, this function returns
    /// `Poll::Pending` and arranges for the current task (via
    /// `cx.waker().wake_by_ref()`) to receive a notification when the object can make
    /// progress towards closing.
    ///
    /// # Implementation
    ///
    /// This function may not return errors of kind `WouldBlock` or
    /// `Interrupted`.  Implementations must convert `WouldBlock` into
    /// `Poll::Pending` and either internally retry or convert
    /// `Interrupted` into another error kind.
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
            // assert_eq!(output, [1, 2, 3, 4, 0]);
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
                _ => Vec::new()
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
            let mut output = vec![1, 2, 3, 4, 5];
            let bytes = writer.write_all2(&mut output[..]).await.unwrap();


            assert_eq!(writer.0.get_mut(), &[1, 2, 3, 4, 5]);
        });
    }

    #[test]
    fn test_write_fixed_u32() {
        async_std::task::block_on(async {
            let mut writer = Test(Cursor::new(b"hello world".to_vec()));
            let _result = writer.write_fixed_u32(1751477356).await.unwrap();

            // Binary of `hell` is 17751477356, if write successfully, current
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
            // assert_eq!(output, [1, 2, 3, 4, 0]);
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
