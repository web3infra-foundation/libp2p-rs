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

use crate::service::ControlCommand;
use crate::INotifiee;
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialOrd, Ord)]
pub struct RegId(u32);

impl RegId {
    /// Create a random connection ID.
    pub(crate) fn random() -> Self {
        RegId(rand::random())
    }
}

impl fmt::Display for RegId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq for RegId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

// HashMap insert() required key impl Hash trait
impl std::hash::Hash for RegId {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        hasher.write_u32(self.0);
    }
}
impl nohash_hasher::IsEnabled for RegId {}

pub struct Control {
    tx: mpsc::Sender<ControlCommand>,
}

impl Control {
    pub fn new(tx: mpsc::Sender<ControlCommand>) -> Self {
        Control { tx }
    }

    pub async fn register_notifee(&mut self, noti: INotifiee) -> RegId {
        let (tx, rx) = oneshot::channel();
        let cmd = ControlCommand::RegisterNotifee(noti, tx);
        self.tx.send(cmd).await.expect("send RegisterNotifee failed");
        rx.await.expect("recv RegisterNotifee failed")
    }

    pub async fn unregister_notifee(&mut self, id: RegId) {
        let _ = self.tx.send(ControlCommand::UnregisterNotifee(id)).await;
    }
}
