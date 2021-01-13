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

use futures::{channel::mpsc, SinkExt, StreamExt};
use std::future::Future;
use std::num::NonZeroUsize;

use async_std::task;

/// The task limiter could be used to limit the maximum count of running task
/// in parallel.
pub(crate) struct TaskLimiter {
    parallelism: NonZeroUsize,
    tx: mpsc::Sender<()>,
    rx: mpsc::Receiver<()>,
    handles: Vec<task::JoinHandle<()>>,
}

impl TaskLimiter {
    pub(crate) fn new(parallelism: NonZeroUsize) -> Self {
        let (mut tx, rx) = mpsc::channel(parallelism.get());
        for _ in 0..parallelism.get() {
            tx.try_send(()).expect("must be ok");
        }

        Self {
            parallelism,
            tx,
            rx,
            handles: vec![],
        }
    }

    pub(crate) async fn run<F, T>(&mut self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        self.rx.next().await.expect("must be Some");
        let mut tx = self.tx.clone();
        let handle = task::spawn(async move {
            let _ = future.await;
            let _ = tx.send(()).await;
        });

        self.handles.push(handle);
    }

    pub(crate) async fn wait(self) -> usize {
        let count = self.handles.len();
        for h in self.handles {
            h.await;
        }
        count
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;
    use std::sync::Arc;

    #[test]
    fn test_task_limiter() {
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        block_on(async move {
            for _ in 0..10 {
                let count = count.clone();
                limiter
                    .run(async move {
                        count.fetch_add(1, SeqCst);
                    })
                    .await;
            }

            let c = limiter.wait().await;
            assert_eq!(c, 10);
            assert_eq!(count.load(SeqCst), 10);
        });
    }
}
