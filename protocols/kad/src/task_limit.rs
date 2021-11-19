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
use futures::future::BoxFuture;
use std::collections::VecDeque;

#[derive(Clone, Default)]
struct Termination(Arc<AtomicBool>);

impl Termination {
    fn terminate(&self) {
        self.0.store(false, SeqCst);
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
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub parallelism: usize,
    pub accepted: usize,
    pub spawned: usize,
    pub executed: usize,
    pub crashed: usize,
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

    fn snapshot(self: &Arc<Self>) -> Snapshot {
        let parallelism = self.parallelism;
        let accepted = self.accepted.load(SeqCst);
        let spawned = self.spawned.load(SeqCst);
        let executed = self.executed.load(SeqCst);
        let crashed = self.crashed.load(SeqCst);
        let running = spawned - executed - crashed;

        Snapshot {
            parallelism,
            accepted,
            spawned,
            executed,
            crashed,
            running,
        }
    }
}

struct Guard(Option<(mpsc::Sender<()>, Arc<Stat>)>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some((mut sender, stat)) = self.0.take() {
            let _ = sender.try_send(());
            stat.crashed();
        }
    }
}

/// The runtime limiter could be used to limit the maximum count of running runtime
/// in parallel.
pub struct TaskLimiter<T> {
    stat: Arc<Stat>,

    terminator: (Termination, mpsc::Sender<()>),
    enqueue: mpsc::UnboundedSender<BoxFuture<'static, ()>>,

    handle: task::TaskHandle<()>,

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

            let mut handles = VecDeque::with_capacity(parallelism);

            loop {
                if termination.terminated() {
                    // ignore the fut in the queue
                    break;
                }
                if let Some(fut) = dequeue.next().await {
                    if token_acquire.next().await.is_none() {
                        break;
                    }
                    if termination.terminated() {
                        // ignore the fut
                        break;
                    }
                    let token_release = token_release.clone();
                    let stat = stat_clone.clone();

                    let handle = task::spawn(async move {
                        stat.spawned();
                        let mut guard = Guard(Some((token_release, stat)));
                        fut.await;
                        let (mut token_release, stat) = guard.0.take().unwrap();
                        token_release.send(()).await.unwrap();
                        stat.executed();
                    });

                    let _ignore = handles.pop_back(); // if some, must executed or panic

                    handles.push_front(handle);
                } else {
                    break;
                }
            }

            for handle in handles {
                handle.cancel().await;
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
            handle,
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

    pub async fn shutdown(mut self) {
        // terminate
        self.terminator.0.terminate();
        self.terminator.1.close_channel();
        self.enqueue.close_channel();
        self.handle.await;
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
}
