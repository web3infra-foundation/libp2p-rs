// Copyright 2021 Netwarps Ltd.
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

use futures::FutureExt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_std::future;
use async_std::task;
use async_std::task::JoinHandle;

#[derive(Debug)]
pub struct TaskHandle<T>(JoinHandle<T>);

impl<T> TaskHandle<T> {
    /// Cancels the runtime immediately, then awaits it. The cancelled runtime might complete
    /// normally with `Some()` or most likely it returns `None`.
    pub async fn cancel(self) -> Option<T> {
        self.0.cancel().await
    }

    /// Waits for the runtime to complete. The runtime will complete normally with `Some()` in
    /// most cases, or it returns `None` if it gets cancelled for some reason.
    ///
    /// This method is actually the Future implemented by itself.
    pub async fn wait(self) -> Option<T> {
        self.await
    }
}

impl<T> Future for TaskHandle<T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_unpin(cx).map(Some)
    }
}

/// Spawns a runtime and blocks the current thread on its result.
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    task::block_on(future)
}

/// Spawns a runtime.
///
/// The returned TaskHandle can be used to terminate and wait for its termination.
///
/// Note: the output of the future must be ().
pub fn spawn<F, T>(future: F) -> TaskHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let h = task::spawn(async move { future.await });
    TaskHandle(h)
}

/// Sleeps for the specified amount of time.
pub async fn sleep(dur: Duration) {
    task::sleep(dur).await
}

/// Awaits a future or times out after a duration of time.
pub async fn timeout<F, T>(dur: Duration, f: F) -> Result<T, ()>
where
    F: Future<Output = T>,
{
    future::timeout(dur, f).await.map_err(|_| ())
}
