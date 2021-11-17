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
use futures::channel::oneshot;

use crate::KadError;
use libp2prs_runtime::task;

/// The runtime limiter could be used to limit the maximum count of running runtime
/// in parallel.
pub(crate) struct TaskLimiter {
    parallelism: NonZeroUsize,
    tx: mpsc::Sender<()>,
    rx: mpsc::Receiver<()>,
    handles: Vec<task::TaskHandle<Result<()>>>,
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

    pub(crate) async fn run<F>(&mut self, future: F)
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        self.rx.next().await.expect("must be Some");
        let mut tx = self.tx.clone();
        let handle = task::spawn(async move {
            let r = future.await;
            let _ = tx.send(()).await;
            r
        });

        self.handles.push(handle);
    }

    pub(crate) async fn wait(self) -> (usize, usize) {
        let count = self.handles.len();
        let mut success = 0;
        for h in self.handles {
            if h.await.map_or(false, |r| r.is_ok()) {
                success += 1;
            }
        }
        (count, success)
    }
}

type Result<T> = std::result::Result<T, KadError>;

#[derive(Debug)]
pub enum TaskLimiterCommand {
    GetToken(oneshot::Sender<Result<()>>),
    PutToken,
}

/// The runtime limiter could be used to limit the maximum count of running runtime
/// in parallel.
pub struct LimitedTaskMgr {
    parallelism: NonZeroUsize,
    tx_token: mpsc::Sender<()>,
    tx: mpsc::UnboundedSender<()>,
    handles: Vec<task::TaskHandle<Result<()>>>,
}

impl LimitedTaskMgr {
    pub fn new(parallelism: NonZeroUsize) -> Self {
        let (tx_token, mut rx_token) = mpsc::channel(parallelism.get());
        let (tx, mut rx) = mpsc::unbounded();

        // start the background task manager
        task::spawn(async move {
            loop {
                match rx.next().await {
                    Some(_) => {
                        log::trace!("received job done notification msg");
                        rx_token.next().await;
                    }
                    None => {
                        log::debug!("LimitedTaskMgr channel closed...");
                        return Ok::<(), KadError>(())
                    }
                }
            }
        });

        Self {
            parallelism,
            tx_token,
            tx,
            handles: vec![],
        }

    }

    pub fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = Result<()>> + Send + 'static,
    {
        let mut tx_token = self.tx_token.clone();
        let mut tx = self.tx.clone();
        let handle = task::spawn(async move {
            let _ = tx_token.send(()).await;
            let r = future.await;
            let _ = tx.send(()).await;
            r
        });

        self.handles.push(handle);
    }

    pub async fn wait(self) -> (usize, usize) {
        let count = self.handles.len();
        let mut success = 0;
        for h in self.handles {
            if h.await.map_or(false, |r| r.is_ok()) {
                success += 1;
            }
        }
        (count, success)
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
    use std::time::Duration;

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
                        Ok::<(), KadError>(())
                    })
                    .await;
            }

            let c = limiter.wait().await;
            assert_eq!(c.0, 10);
            assert_eq!(count.load(SeqCst), 10);
        });
    }

    #[test]
    fn test_task_limiter2() {
        let mut limiter = LimitedTaskMgr::new(NonZeroUsize::new(2).unwrap());
        let count = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            limiter.spawn(async move {
                task::sleep(Duration::from_secs(1)).await;
                Ok(())
            });
        }

        assert_eq!(limiter.handles.len(), 10);


        // block_on(async move {
        //     for _ in 0..10 {
        //         let count = count.clone();
        //         limiter
        //             .run(async move {
        //                 count.fetch_add(1, SeqCst);
        //                 Ok::<(), KadError>(())
        //             })
        //             .await;
        //     }
        //
        //     let c = limiter.wait().await;
        //     assert_eq!(c.0, 10);
        //     assert_eq!(count.load(SeqCst), 10);
        // });
    }
}
