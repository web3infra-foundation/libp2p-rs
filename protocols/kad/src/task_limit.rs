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

use futures::channel::oneshot;
use futures::{channel::mpsc, SinkExt, StreamExt, FutureExt};
use std::future::Future;
use std::num::NonZeroUsize;

use crate::KadError;
use libp2prs_runtime::task;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst}};
use futures::future::{BoxFuture, Either, select};
use std::collections::{HashMap};

#[derive(Clone, Default)]
struct Termination(Arc<AtomicBool>);

impl Termination {
    fn terminate(&self) {
        self.0.store(true, SeqCst);
    }

    fn terminated(&self) -> bool {
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

    fn snapshot(self: &Arc<Self>) -> Snapshot {
        let parallelism = self.parallelism;
        let accepted = self.accepted.load(SeqCst);
        let spawned = self.spawned.load(SeqCst);
        let executed = self.executed.load(SeqCst);
        let crashed = self.crashed.load(SeqCst);
        let canceled = self.canceled.load(SeqCst);
        let running = spawned - executed - crashed - canceled;

        Snapshot {
            parallelism,
            accepted,
            spawned,
            executed,
            crashed,
            canceled,
            running,
        }
    }
}

struct Guard(Option<(usize, mpsc::Sender<(usize, bool)>)>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some((id, mut sender)) = self.0.take() {
            let _ = sender.try_send((id, false));
        }
    }
}

/// The runtime limiter could be used to limit the maximum count of running runtime
/// in parallel.
pub struct TaskLimiter<T> {
    stat: Arc<Stat>,

    terminator: (Termination, mpsc::Sender<()>),
    enqueue: mpsc::UnboundedSender<BoxFuture<'static, ()>>,

    handle: Option<task::TaskHandle<()>>,

    // waits: Option<mpsc::UnboundedReceiver<T>>,
    waits: Option<Vec<oneshot::Receiver<T>>>,
}

impl<T> TaskLimiter<T>
where
    T: Send + 'static,
{
    pub fn new(parallelism: NonZeroUsize, waitable: bool) -> Self {

        let parallelism = parallelism.get();

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

            let mut handles = HashMap::with_capacity(parallelism);
            let mut task_id = 0;
            let (done_tx, mut done_rx) = mpsc::channel(parallelism);

            'outer: loop {
                if termination.terminated() {
                    // ignore the fut in the queue
                    break;
                }
                if let Some(fut) = dequeue.next().await {
                    loop {
                        match select(token_acquire.next(), done_rx.next()).await {
                            Either::Left((token, _)) => {
                                if token.is_none() {
                                    break 'outer;
                                }
                                if termination.terminated() {
                                    // ignore the fut
                                    break 'outer;
                                }
                                // let token_release = token_release.clone();
                                task_id += 1;
                                let done_tx = done_tx.clone();
                                let stat = stat_clone.clone();

                                let handle = task::spawn(async move {
                                    stat.spawned();
                                    let mut guard = Guard(Some((task_id, done_tx)));
                                    fut.await;
                                    let (id, mut done) = guard.0.take().unwrap();
                                    let _ = done.send((id, true)).await;
                                });

                                handles.insert(task_id, handle);
                                break;
                            },
                            Either::Right((ret, _)) => {
                                let (id, executed) = ret.unwrap();
                                handles.remove(&id);
                                let _ = token_release.send(()).await;
                                if executed {
                                    stat_clone.executed();
                                } else {
                                    stat_clone.crashed();
                                }
                            }
                        }
                    }
                } else {
                    break;
                }
            }

            log::info!("exiting task limiter");

            while let Ok(Some((id, executed))) = done_rx.try_next() {
                handles.remove(&id);
                if executed {
                    stat_clone.executed();
                } else {
                    stat_clone.crashed();
                }
            }

            for (_, handle) in handles {
                let _ = handle.cancel().await;
            }

            // collect canceled
            while let Ok(Some((_id, executed))) = done_rx.try_next() {
                debug_assert!(!executed);
                stat_clone.canceled();
            }
        });

        let waits = {
            if waitable {
                Some(Default::default())
            } else {
                None
            }
        };

        Self {
            stat,
            terminator,
            enqueue,
            handle: Some(handle),
            waits,
        }
    }

    pub fn spawn<Fut>(&mut self, fut: Fut)
    where
        Fut: Future<Output = T> + Send + 'static,
    {
        let fut = {
            if let Some(waits) = &mut self.waits {
                let (fin, wait) = oneshot::channel();
                waits.push(wait);
                async move {
                    let ret = fut.await;
                    let _ignore = fin.send(ret);
                }.boxed()
            } else {
                fut.map(|_| ()).boxed()
            }
        };

        self.enqueue.unbounded_send(fut).expect("send");
        self.stat.accepted();
    }

    pub async fn shutdown(&mut self) {
        // terminate
        self.terminator.0.terminate();
        self.terminator.1.close_channel();
        self.enqueue.close_channel();
        self.handle.take().unwrap().await;
    }

    pub async fn wait<P>(self, predicate: P) -> (usize, usize)
        where
            P: Fn(T) -> bool,
    {
        let waits = self.waits.unwrap_or_else(|| panic!("unwaitable"));

        let count = waits.len();
        let mut success = 0;
        for w in waits {
            if w.await.map_or(false, |ret| predicate(ret)) {
                success += 1;
            }
        }
        (count, success)
    }

    pub fn stat(&self) -> Snapshot {
        self.stat.snapshot()
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
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap(), true);
        let count = Arc::new(AtomicUsize::new(0));
        block_on(async move {
            for _ in 0..10 {
                let count = count.clone();
                limiter
                    .spawn(async move {
                        count.fetch_add(1, SeqCst);
                        Ok::<(), KadError>(())
                    });
            }

            let c = limiter.wait(|ret| ret.is_ok()).await;
            assert_eq!(c.0, 10);
            assert_eq!(count.load(SeqCst), 10);
        });
    }

    #[test]
    fn test_crash() {
        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap(), true);
        block_on(async move {

            limiter.spawn(async move {
                Ok(())
            });

            limiter.spawn(async move {
                panic!("test crash");
            });

            limiter.spawn(async move {
                Err(KadError::Internal)
            });

            task::sleep(Duration::from_secs(2)).await;

            log::info!("{:?}", limiter.stat());

            let c = limiter.wait(|ret| ret.is_ok()).await;
            assert_eq!(c.0, 3);
            assert_eq!(c.1, 1);
        });
    }

    #[test]
    fn test_limit() {

        env_logger::builder().filter_level(log::LevelFilter::Info).is_test(true).init();
        let mut limiter = TaskLimiter::new(NonZeroUsize::new(3).unwrap(), false);
        block_on(async move {

            let (tx, mut rx) = mpsc::channel(0);

            for i in 1..=5 {
                let mut tx = tx.clone();
                limiter.spawn(async move {
                    log::info!("running {}", i);
                    tx.send(()).await.unwrap();
                    log::info!("done {}", i);
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

            limiter.shutdown().await;

            // wait guard drop
            task::sleep(Duration::from_millis(100)).await;


            let stat = limiter.stat();
            log::info!("{:?}", stat);

            assert_eq!(stat.accepted, 5, "not equal accepted2");
            assert_eq!(stat.spawned, 4, "not equal spawned2");
            assert_eq!(stat.executed, 1, "not equal executed2");
            assert_eq!(stat.crashed, 0, "not equal crashed");
            assert_eq!(stat.canceled, 3, "not equal canceled");
            assert_eq!(stat.running, 0, "not equal running3");
        });
    }
}
