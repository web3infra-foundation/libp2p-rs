mod ext;
mod copy;

use async_trait::async_trait;
use futures::prelude::*;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::io;
use std::io::ErrorKind;

pub use ext::ReadExt2;
pub use copy::copy;

/// Read Trait for async/wait
///
#[async_trait]
pub trait Read2 {
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
    /// # blocking::block_on(async {
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
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Reads the exact number of bytes requested.
    ///
    /// On success, returns the total number of bytes read.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # blocking::block_on(async {
    /// let mut reader = io::Cursor::new(&b"hello");
    /// let mut contents = vec![0; 3];
    ///
    /// reader.read_exact(&mut contents).await?;
    /// assert_eq!(contents, b"hel");
    /// # std::io::Result::Ok(()) });
    /// ```
    async fn read_exact2<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<()> {
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
}

/// Write Trait for async/wait
///
#[async_trait]
pub trait Write2 {
    /// Attempt to write bytes from `buf` into the object.
    ///
    /// On success, returns `Ok(num_bytes_written)`.
    /// Otherwise, returns io::Error
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize>;
    /// Attempt to write the entire contents of data into object.
    ///
    /// The operation will not complete until all the data has been written.
    ///
    async fn write_all2(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut buf_piece = buf;
        while !buf_piece.is_empty() {
            let n = self.write2(buf).await?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }

            let (_, rest) = buf_piece.split_at(n);
            buf_piece = rest;
        }
        Ok(())
    }

    /// Attempt to flush the object, ensuring that any buffered data reach
    /// their destination.
    ///
    /// On success, returns `Ok(())`.
    async fn flush2(&mut self) -> io::Result<()>;

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
    async fn close2(&mut self) -> io::Result<()>;
}

#[async_trait]
impl<T: AsyncRead + Unpin + Send> Read2 for T {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = AsyncReadExt::read(self, buf).await?;
        Ok(n)
    }
}

#[async_trait]
impl<T: AsyncWrite + Unpin + Send> Write2 for T {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        AsyncWriteExt::write(self, buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        AsyncWriteExt::flush(self).await
    }

    async fn close2(&mut self) -> io::Result<()> {
        AsyncWriteExt::close(self).await
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split() {
        async_std::task::block_on(async {
            use futures::io::{self, AsyncReadExt, Cursor};

            struct Test(Cursor<Vec<u8>>);

            #[async_trait]
            impl Read2 for Test {
                async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                    self.0.read(buf).await
                }
            }

            #[async_trait]
            impl Write2 for Test {
                async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
                    self.0.write(buf).await
                }

                async fn flush2(&mut self) -> io::Result<()> {
                    self.0.flush().await
                }

                async fn close2(&mut self) -> io::Result<()> {
                    self.0.close().await
                }
            }

            // Note that for `Cursor` the read and write halves share a single
            // seek position. This may or may not be true for other types that
            // implement both `AsyncRead` and `AsyncWrite`.

            let reader = Cursor::new(vec![1u8,2,3,4]);
            let mut buffer = Test(Cursor::new(vec![0, 0, 0, 0, 5, 6, 7, 8]));
            let mut writer = Cursor::new(vec![0u8; 5]);

            {
                let (buffer_reader, buffer_writer) = buffer.split();
                copy(reader, buffer_writer).await?;
                copy(buffer_reader, &mut writer).await?;
            }

            // assert_eq!(buffer.into_inner(), [1, 2, 3, 4, 5, 6, 7, 8]);
            // assert_eq!(writer.into_inner(), [5, 6, 7, 8, 0]);
            Ok::<(), Box<dyn std::error::Error>>(())
        }).unwrap();
    }
}
 */