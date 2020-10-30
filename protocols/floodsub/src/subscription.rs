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

use crate::protocol::FloodsubMessage;
use crate::Topic;
use futures::channel::mpsc;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialOrd, Ord)]
pub struct SubId(u32);

impl SubId {
    /// Create a random connection ID.
    pub(crate) fn random() -> Self {
        SubId(rand::random())
    }
}

impl fmt::Display for SubId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq for SubId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

// HashMap insert() required key impl Hash trait
impl std::hash::Hash for SubId {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        hasher.write_u32(self.0);
    }
}
impl nohash_hasher::IsEnabled for SubId {}

pub struct Subscription {
    pub id: SubId,
    pub topic: Topic,
    pub ch: mpsc::UnboundedReceiver<FloodsubMessage>,
    pub cancel: mpsc::UnboundedSender<Subscription>,
}

impl Subscription {
    pub fn new(
        id: SubId,
        topic: Topic,
        ch: mpsc::UnboundedReceiver<FloodsubMessage>,
        cancel: mpsc::UnboundedSender<Subscription>,
    ) -> Self {
        Subscription { id, topic, ch, cancel }
    }
}
