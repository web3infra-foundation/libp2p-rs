
use std::io;
use async_trait::async_trait;

/// Read Trait for async/wait
///
#[async_trait]
pub trait Read {
    /// Attempt to read bytes from underlying object.
    ///
    /// On success, returns `Ok(Vec<u8>)`.
    /// Otherwise, returns io::Error
    async fn read(&mut self) -> io::Result<Vec<u8>>;
}

/// Write Trait for async/wait
///
#[async_trait]
pub trait Write {
    /// Attempt to write bytes from `buf` into the object.
    ///
    /// On success, returns `Ok(num_bytes_written)`.
    /// Otherwise, returns io::Error
    async fn write(&mut self, buf: &Vec<u8>) -> io::Result<()>;
    /// Attempt to flush the object, ensuring that any buffered data reach
    /// their destination.
    ///
    /// On success, returns `Ok(())`.
    async fn flush(&mut self) -> io::Result<()>;

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
    async fn close(&mut self) -> io::Result<()>;
}