use async_trait::async_trait;
use futures::lock::BiLock;
use std::{fmt, future::Future, io, task::Poll};

use super::{ReadEx, WriteEx};

#[derive(Debug)]
pub struct ReadHalf<T> {
    handle: BiLock<T>,
}

#[async_trait]
impl<T: ReadEx + Unpin> ReadEx for ReadHalf<T> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        futures::future::poll_fn(|cx| {
            let mut lock = futures::ready!(self.handle.poll_lock(cx));
            let t = &mut *lock;
            let fut = t.read2(buf);
            futures::pin_mut!(fut);
            let ret = futures::ready!(fut.poll(cx));
            Poll::Ready(ret)
        })
        .await
    }
}

/*
/// The readable half of an object returned from `AsyncRead::split`.
#[derive(Debug)]
pub struct ReadHalf2<T> {
    handle: BiLock<T>,
    fut: Option<Pin<Box<dyn Future<Output=io::Result<usize>> + Send + Unpin>>>,
}

impl<T> ReadHalf2<T> {
    pub fn new(lock: BiLock<T>) -> Self {
        ReadHalf2 {
            handle: lock,
            fut: None,
        }
    }
}

#[async_trait]
impl<T: ReadEx + Unpin> ReadEx for ReadHalf2<T> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        futures::future::poll_fn(|cx| {
            let mut lock = futures::ready!(self.handle.poll_lock(cx));
            let t = &mut *lock;
            if self.fut.is_none() {
                self.fut = Some(t.read2(buf));
            }
            let fut = self.fut.take().expect("must not be none");
            match fut.poll(cx) {
                Poll::Pending => {
                    self.fut = Some(fut);
                    Poll::Pending
                },
                Poll::Ready(ret) => {
                    Poll::Ready(ret)
                }
            }
            // futures::ready!(self.fut.as_mut().as_pin_mut().expect("must not be none").poll(cx))
        }).await
    }
}

pub(super) fn split2<T>(t: T) -> (ReadHalf2<T>, WriteHalf<T>)
    where
        T: ReadEx + WriteEx + Unpin
{
    let (a, b) = BiLock::new(t);
    let x = ReadHalf2::new(a);
    (x, WriteHalf { handle: b })
}
 */

/// The writable half of an object returned from `AsyncRead::split`.
#[derive(Debug)]
pub struct WriteHalf<T> {
    handle: BiLock<T>,
}

/*
async fn lock_and_then<'a, T, U, E, F, Fut>(
    lock: &'a BiLock<T>,
    f: F
) -> Result<U, E>
    where
        T: Send + Unpin,
        F: FnMut(&'a mut T) -> Fut,
        Fut: Future<Output = Result<U, E>>,
        Fut: Send,
        Fut::Output: Send,
{
    futures::future::poll_fn(|cx| {
        let mut lock = futures::ready!(lock.poll_lock(cx));
        let t = &mut *lock;
        let fut = f(t);
        futures::pin_mut!(fut);
        let ret = futures::ready!(fut.poll(cx));
        Poll::Ready(ret)
    }).await
}
 */

pub(super) fn split<T>(t: T) -> (ReadHalf<T>, WriteHalf<T>)
where
    T: ReadEx + WriteEx + Unpin,
{
    let (a, b) = BiLock::new(t);
    (ReadHalf { handle: a }, WriteHalf { handle: b })
}

impl<T: Unpin> ReadHalf<T> {
    /// Attempts to put the two "halves" of a split `AsyncRead + AsyncWrite` back
    /// together. Succeeds only if the `ReadHalf<T>` and `WriteHalf<T>` are
    /// a matching pair originating from the same call to `AsyncReadExt::split`.
    pub fn reunite(self, other: WriteHalf<T>) -> Result<T, ReuniteError<T>> {
        self.handle
            .reunite(other.handle)
            .map_err(|err| ReuniteError(ReadHalf { handle: err.0 }, WriteHalf { handle: err.1 }))
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
impl<W: WriteEx + Unpin> WriteEx for WriteHalf<W> {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        // self.handle.lock().await.write2(buf).await
        futures::future::poll_fn(|cx| {
            let mut lock = futures::ready!(self.handle.poll_lock(cx));
            let t = &mut *lock;
            let fut = t.write2(buf);
            futures::pin_mut!(fut);
            let ret = futures::ready!(fut.poll(cx));
            Poll::Ready(ret)
        })
        .await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        // self.handle.lock().await.flush2().await
        futures::future::poll_fn(|cx| {
            let mut lock = futures::ready!(self.handle.poll_lock(cx));
            let t = &mut *lock;
            let fut = t.flush2();
            futures::pin_mut!(fut);
            let ret = futures::ready!(fut.poll(cx));
            Poll::Ready(ret)
        })
        .await
    }

    async fn close2(&mut self) -> io::Result<()> {
        // self.handle.lock().await.close2().await
        futures::future::poll_fn(|cx| {
            let mut lock = futures::ready!(self.handle.poll_lock(cx));
            let t = &mut *lock;
            let fut = t.close2();
            futures::pin_mut!(fut);
            let ret = futures::ready!(fut.poll(cx));
            Poll::Ready(ret)
        })
        .await
    }
}

/// Error indicating a `ReadHalf<T>` and `WriteHalf<T>` were not two halves
/// of a `AsyncRead + AsyncWrite`, and thus could not be `reunite`d.
pub struct ReuniteError<T>(pub ReadHalf<T>, pub WriteHalf<T>);

impl<T> fmt::Debug for ReuniteError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReuniteError").field(&"...").finish()
    }
}

impl<T> fmt::Display for ReuniteError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tried to reunite a ReadHalf and WriteHalf that don't form a pair")
    }
}

impl<T: core::any::Any> std::error::Error for ReuniteError<T> {}
