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

use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc, Mutex,
    },
};

use futures::{channel::mpsc, future::BoxFuture, FutureExt, StreamExt};

use crate::task::{self, TaskHandle};

/// The runtime limiter could be used to limit the maximum count of running runtime
/// in parallel.
pub struct TaskLimiter {
    stat: Arc<Stat>,
    terminator: (Termination, mpsc::Sender<()>),
    enqueue: mpsc::UnboundedSender<BoxFuture<'static, bool>>,
    handle: task::TaskHandle<Snapshot>,
}

impl TaskLimiter {
    pub fn new(parallelism: NonZeroUsize) -> Self {
        let parallelism = parallelism.get();

        log::debug!("task-limiter: starting with parallelism={}, ...", parallelism);

        let (enqueue, mut dequeue) = mpsc::unbounded::<BoxFuture<_>>();
        let (mut token_release, mut token_acquire) = mpsc::channel(parallelism);

        // pre-filled token
        for _ in 0..parallelism {
            token_release.try_send(()).unwrap();
        }

        let termination = Termination::default();
        let terminator = (termination.clone(), token_release.clone());
        let stat = Stat::new(parallelism);

        let stat_clone = stat.clone();
        let handle = task::spawn(async move {
            let handles = Arc::new(Mutex::new(HashMap::with_capacity(5 * parallelism)));
            let mut task_id = 0;
            loop {
                // terminated ??
                if termination.is_terminated() {
                    // ignore the fut in the queue
                    break;
                }
                // channel is closed ?
                let fut = if let Some(fut) = dequeue.next().await {
                    fut
                } else {
                    break;
                };

                // recheck if it is terminated
                if token_acquire.next().await.is_none() || termination.is_terminated() {
                    // ignore the fut
                    break;
                }

                task_id += 1;
                let inner = GuardInner {
                    task_id,
                    handles: handles.clone(),
                    token_release: token_release.clone(),
                    stat: stat_clone.clone(),
                    termination: termination.clone(),
                };

                // clone the stat and move it into the task
                let stat = stat_clone.clone();
                let task = task::spawn(async move {
                    stat.spawned();
                    let mut guard = Guard(Some(inner));
                    if fut.await {
                        stat.success();
                    }
                    guard.0.take().unwrap().feedback(true);
                });

                // put the spawned task handle into the task map
                handles.lock().unwrap().insert(task_id, task);
            }

            log::debug!("task-limiter: exiting...");

            let handles = {
                let mut map = handles.lock().unwrap();

                let mut handles = Vec::with_capacity(map.len());
                let keys = map.keys().cloned().collect::<Vec<_>>();
                for key in keys {
                    handles.push(map.remove(&key).unwrap());
                }
                handles
            };

            for handle in handles {
                if termination.is_terminated() {
                    let _ = handle.cancel().await;
                } else {
                    handle.await;
                }
            }

            // return the snapshot of statistics
            stat_clone.snapshot()
        });

        Self {
            stat,
            terminator,
            enqueue,
            handle,
        }
    }

    pub fn spawn<T, E, Fut>(&mut self, fut: Fut)
    where
        T: Send + 'static,
        E: Send + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
    {
        self.enqueue.unbounded_send(fut.map(|ret| ret.is_ok()).boxed()).expect("send");
        self.stat.accepted();
    }

    pub async fn shutdown(mut self) -> Snapshot {
        // terminate
        self.terminator.0.terminate();
        self.terminator.1.close_channel();
        self.enqueue.close_channel();
        self.handle.await.unwrap_or_default()
    }

    pub async fn wait(self) -> Snapshot {
        self.enqueue.close_channel();
        self.handle.await.unwrap_or_default()
    }

    pub fn stat(&self) -> Snapshot {
        self.stat.snapshot()
    }
}

#[derive(Clone, Default)]
struct Termination(Arc<AtomicBool>);

impl Termination {
    fn terminate(&self) {
        self.0.store(true, SeqCst);
    }

    fn is_terminated(&self) -> bool {
        self.0.load(SeqCst)
    }
}

#[derive(Default)]
struct Stat {
    parallelism: usize,
    accepted: AtomicUsize,
    spawned: AtomicUsize,
    executed: AtomicUsize,
    crashed: AtomicUsize,
    canceled: AtomicUsize,
    succeeded: AtomicUsize,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub parallelism: usize,
    pub accepted: usize,
    pub spawned: usize,
    pub executed: usize,
    pub crashed: usize,
    pub canceled: usize,
    pub running: usize,
    pub succeeded: usize,
}

impl Stat {
    fn new(parallelism: usize) -> Arc<Stat> {
        Arc::new(Stat {
            parallelism,
            ..Default::default()
        })
    }

    fn accepted(self: &Arc<Self>) {
        self.accepted.fetch_add(1, SeqCst);
    }

    fn spawned(self: &Arc<Self>) {
        self.spawned.fetch_add(1, SeqCst);
    }

    fn executed(self: &Arc<Self>) {
        self.executed.fetch_add(1, SeqCst);
    }

    fn crashed(self: &Arc<Self>) {
        self.crashed.fetch_add(1, SeqCst);
    }

    fn canceled(self: &Arc<Self>) {
        self.canceled.fetch_add(1, SeqCst);
    }

    fn success(self: &Arc<Self>) {
        self.succeeded.fetch_add(1, SeqCst);
    }

    fn snapshot(self: &Arc<Self>) -> Snapshot {
        let parallelism = self.parallelism;
        let accepted = self.accepted.load(SeqCst);
        let spawned = self.spawned.load(SeqCst);
        let executed = self.executed.load(SeqCst);
        let crashed = self.crashed.load(SeqCst);
        let canceled = self.canceled.load(SeqCst);
        let succeeded = self.succeeded.load(SeqCst);
        let running = spawned - executed - crashed - canceled;

        Snapshot {
            parallelism,
            accepted,
            spawned,
            executed,
            crashed,
            canceled,
            running,
            succeeded,
        }
    }
}

/// prevent future task panic from causing token to be swallowed
struct Guard(Option<GuardInner>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(inner) = self.0.take() {
            inner.feedback(false);
        }
    }
}

struct GuardInner {
    task_id: usize,
    token_release: mpsc::Sender<()>,
    handles: Arc<Mutex<HashMap<usize, TaskHandle<()>>>>,
    stat: Arc<Stat>,
    termination: Termination,
}

impl GuardInner {
    fn feedback(mut self, normal: bool) {
        let _ignore = self.token_release.try_send(());
        self.handles.lock().unwrap().remove(&self.task_id);
        if normal {
            self.stat.executed();
        } else {
            if self.termination.is_terminated() {
                self.stat.canceled();
            } else {
                self.stat.crashed();
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use futures::SinkExt;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_task_limiter() {
        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();

        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        block_on(async move {
            for _ in 0..10 {
                let count = count.clone();
                limiter.spawn(async move {
                    count.fetch_add(1, SeqCst);
                    Ok::<(), ()>(())
                });
            }

            let c = limiter.wait().await;
            assert_eq!(c.executed, 10, "total");
            assert_eq!(count.load(SeqCst), 10, "count");
        });
    }

    async fn crash() -> std::result::Result<(), ()> {
        panic!("test crash");
    }

    #[test]
    fn test_crash() {
        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap());
        block_on(async move {
            limiter.spawn(async move { Ok::<(), ()>(()) });

            limiter.spawn(crash());

            limiter.spawn(async move { Err::<(), ()>(()) });

            task::sleep(Duration::from_millis(100)).await;

            log::info!("{:?}", limiter.stat());

            let c = limiter.wait().await;
            assert_eq!(c.spawned, 3);
            assert_eq!(c.executed, 2);
            assert_eq!(c.succeeded, 1);
            assert_eq!(c.crashed, 1);
        });
    }

    #[test]
    fn test_limit() {
        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap());
        block_on(async move {
            let (tx, mut rx) = mpsc::channel(0);

            for i in 1..=5 {
                let mut tx = tx.clone();
                limiter.spawn(async move {
                    log::info!("running {}", i);
                    tx.send(()).await.unwrap();
                    log::info!("done {}", i);
                    Ok::<(), ()>(())
                });
            }

            // wait task to spawn
            task::sleep(Duration::from_millis(100)).await;

            let stat = limiter.stat();
            assert_eq!(stat.accepted, 5, "not equal accepted");
            assert_eq!(stat.spawned, 3, "not equal spawned");
            assert_eq!(stat.running, 3, "not equal running");

            let _ = rx.next().await.unwrap();

            // wait task to spawn
            task::sleep(Duration::from_millis(100)).await;

            let stat = limiter.stat();
            assert_eq!(stat.executed, 1, "not equal executed");
            assert_eq!(stat.running, 3, "not equal running2");

            let stat = limiter.shutdown().await;

            // wait guard drop
            task::sleep(Duration::from_millis(100)).await;

            log::info!("{:?}", stat);

            assert_eq!(stat.accepted, 5, "not equal accepted2");
            assert_eq!(stat.spawned, 4, "not equal spawned2");
            assert_eq!(stat.executed, 1, "not equal executed2");
            assert_eq!(stat.crashed, 0, "not equal crashed");
            assert_eq!(stat.canceled, 3, "not equal canceled");
            assert_eq!(stat.running, 0, "not equal running3");
        });
    }

    #[test]
    fn test_finish() {
        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(1).unwrap());
        block_on(async move {
            let (tx, mut rx) = mpsc::channel(0);

            for i in 1..=2 {
                let mut tx = tx.clone();
                limiter.spawn(async move {
                    log::info!("running {}", i);
                    tx.send(()).await.unwrap();
                    log::info!("done {}", i);
                    Ok::<(), ()>(())
                });
            }

            // wait task to spawn
            task::sleep(Duration::from_millis(100)).await;

            let stat = limiter.stat();
            assert_eq!(stat.accepted, 2, "not equal accepted");
            assert_eq!(stat.spawned, 1, "not equal spawned");
            assert_eq!(stat.running, 1, "not equal running");

            let _ = rx.next().await.unwrap();
            // wait task to spawn
            task::sleep(Duration::from_millis(100)).await;
            let stat = limiter.stat();
            assert_eq!(stat.executed, 1, "not equal executed");
            assert_eq!(stat.running, 1, "not equal running2");

            let _ = rx.next().await.unwrap();
            // wait task to spawn
            task::sleep(Duration::from_millis(100)).await;
            let stat = limiter.stat();
            assert_eq!(stat.executed, 2, "not equal executed");
            assert_eq!(stat.running, 0, "not equal running2");
        });
    }
}
