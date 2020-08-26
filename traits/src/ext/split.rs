use std::{
    fmt,
    io,
};

use async_trait::async_trait;

use crate::{Read2, Write2};
use super::bilock::BiLock;

/// The readable half of an object returned from `Read2::split`.
#[derive(Debug)]
pub struct ReadHalf<T> {
    handle: BiLock<T>,
}

/// The writable half of an object returned from `Read2::split`.
#[derive(Debug)]
pub struct WriteHalf<T> {
    handle: BiLock<T>,
}

pub(super) fn split<T: Read2 + Write2>(t: T) -> (ReadHalf<T>, WriteHalf<T>) {
    let (a, b) = BiLock::new(t);
    (ReadHalf { handle: a }, WriteHalf { handle: b })
}

impl<T: Unpin> ReadHalf<T> {
    /// Attempts to put the two "halves" of a split `AsyncRead + AsyncWrite` back
    /// together. Succeeds only if the `ReadHalf<T>` and `WriteHalf<T>` are
    /// a matching pair originating from the same call to `AsyncReadExt::split`.
    pub fn reunite(self, other: WriteHalf<T>) -> Result<T, ReuniteError<T>> {
        self.handle.reunite(other.handle).map_err(|err| {
            ReuniteError(ReadHalf { handle: err.0 }, WriteHalf { handle: err.1 })
        })
    }
}

impl<T: Unpin> WriteHalf<T> {
    /// Attempts to put the two "halves" of a split `AsyncRead + AsyncWrite` back
    /// together. Succeeds only if the `ReadHalf<T>` and `WriteHalf<T>` are
    /// a matching pair originating from the same call to `AsyncReadExt::split`.
    pub fn reunite(self, other: ReadHalf<T>) -> Result<T, ReuniteError<T>> {
        other.reunite(self)
    }
}

#[async_trait]
impl<R: Read2 + Send> Read2 for ReadHalf<R> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.handle.lock().await.read2(buf).await
    }
}

#[async_trait]
impl<W: Write2 + Send> Write2 for WriteHalf<W> {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.handle.lock().await.write2(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.handle.lock().await.flush2().await
    }

    async fn close2(&mut self) -> io::Result<()> {
        self.handle.lock().await.close2().await
    }
}

/// Error indicating a `ReadHalf<T>` and `WriteHalf<T>` were not two halves
/// of a `AsyncRead + AsyncWrite`, and thus could not be `reunite`d.
pub struct ReuniteError<T>(pub ReadHalf<T>, pub WriteHalf<T>);

impl<T> fmt::Debug for ReuniteError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReuniteError")
            .field(&"...")
            .finish()
    }
}

impl<T> fmt::Display for ReuniteError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tried to reunite a ReadHalf and WriteHalf that don't form a pair")
    }
}

impl<T: core::any::Any> std::error::Error for ReuniteError<T> {}
