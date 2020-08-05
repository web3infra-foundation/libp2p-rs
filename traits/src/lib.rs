use async_trait::async_trait;
use futures::prelude::*;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::io;
use std::io::ErrorKind;

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
    async fn read2<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>;

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
    async fn write2(&mut self, buf: &[u8]) -> io::Result<()>;
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
    async fn read2<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize> {
        let n = AsyncReadExt::read(self, buf).await?;
        Ok(n)
    }
}

#[async_trait]
impl<T: AsyncWrite + Unpin + Send> Write2 for T {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<()> {
        AsyncWriteExt::write_all(self, buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        AsyncWriteExt::flush(self).await
    }

    async fn close2(&mut self) -> io::Result<()> {
        AsyncWriteExt::close(self).await
    }
}
