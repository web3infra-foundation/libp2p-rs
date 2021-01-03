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
