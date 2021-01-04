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
