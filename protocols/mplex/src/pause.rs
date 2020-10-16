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

use futures::{prelude::*, stream::FusedStream};
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Wraps a [`futures::stream::Stream`] and adds the ability to pause it.
///
/// When pausing the stream, any call to `poll_next` will return
/// `Poll::Pending` and the `Waker` will be saved (only the most recent
/// one). When unpaused, the waker will be notified and the next call
/// to `poll_next` can proceed as normal.
#[derive(Debug)]
pub(crate) struct Pausable<S> {
    paused: bool,
    stream: S,
    waker: Option<Waker>,
}

impl<S: Stream + Unpin> Pausable<S> {
    pub(crate) fn new(stream: S) -> Self {
        Pausable {
            paused: false,
            stream,
            waker: None,
        }
    }

    pub(crate) fn is_paused(&mut self) -> bool {
        self.paused
    }

    pub(crate) fn pause(&mut self) {
        self.paused = true
    }

    pub(crate) fn unpause(&mut self) {
        self.paused = false;
        if let Some(w) = self.waker.take() {
            w.wake()
        }
    }

    pub(crate) fn stream(&mut self) -> &mut S {
        &mut self.stream
    }
}

impl<S: Stream + Unpin> Stream for Pausable<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if !self.paused {
            return self.stream.poll_next_unpin(cx);
        }
        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<S: FusedStream + Unpin> FusedStream for Pausable<S> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

#[cfg(test)]
mod tests {
    use super::Pausable;
    use futures::prelude::*;

    #[test]
    fn pause_unpause() {
        // The stream produced by `futures::stream::iter` is always ready.
        let mut stream = Pausable::new(futures::stream::iter(&[1, 2, 3, 4]));
        assert_eq!(Some(Some(&1)), stream.next().now_or_never());
        assert_eq!(Some(Some(&2)), stream.next().now_or_never());
        stream.pause();
        assert_eq!(None, stream.next().now_or_never());
        stream.unpause();
        assert_eq!(Some(Some(&3)), stream.next().now_or_never());
        assert_eq!(Some(Some(&4)), stream.next().now_or_never());
        assert_eq!(Some(None), stream.next().now_or_never()) // end of stream
    }
}
