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

use crate::control::ControlCommand::Heartbeat;
use crate::error::PublishError::InsufficientPeers;
use crate::error::{PublishError, SubscriptionError};
use crate::subscription::Subscription;
use crate::{GossipsubConfig, GossipsubMessage, TopicHash};
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2prs_core::PeerId;
use std::collections::btree_set::BTreeSet;
use std::collections::HashMap;
use std::error::Error;

pub(crate) enum ControlCommand {
    Publish(GossipsubMessage, oneshot::Sender<()>),
    Subscribe(TopicHash, oneshot::Sender<Result<Subscription, SubscriptionError>>),
    Unsubscribed(TopicHash),
    Heartbeat,
    // Ls(oneshot::Sender<Vec<Topic>>),
    // GetPeers(Topic, oneshot::Sender<Vec<PeerId>>)
    GetFanoutPeer(oneshot::Sender<HashMap<TopicHash, BTreeSet<PeerId>>>),
    GetMeshPeer(oneshot::Sender<HashMap<TopicHash, BTreeSet<PeerId>>>),
    _GetKnownTopicByPeer(oneshot::Sender<Vec<TopicHash>>),
}

#[derive(Clone)]
pub struct Control {
    config: GossipsubConfig,
    control_sender: mpsc::UnboundedSender<ControlCommand>,
}

impl Control {
    pub(crate) fn new(control_sender: mpsc::UnboundedSender<ControlCommand>, config: GossipsubConfig) -> Self {
        Control { config, control_sender }
    }
    /// Closes the floodsub main loop.
    pub fn close(&mut self) {
        self.control_sender.close_channel();
    }

    /// Publish publishes data to a given topic.
    pub async fn publish(&mut self, topic: TopicHash, data: impl Into<Vec<u8>>) -> Result<(), PublishError> {
        // unimplemented!()
        let msg = GossipsubMessage {
            source: None,
            data: data.into(),
            sequence_number: Some(rand::random::<u64>()),
            topic,
        };

        let (tx, rx) = oneshot::channel();
        self.control_sender
            .send(ControlCommand::Publish(msg, tx))
            .await
            .map_err(|_| InsufficientPeers)?;

        rx.await.map_err(|_| InsufficientPeers)
    }

    /// Subscribe to messages on a given topic.
    pub async fn subscribe(&mut self, topic: TopicHash) -> Result<Subscription, SubscriptionError> {
        // unimplemented!()
        let (tx, rx) = oneshot::channel();
        let _ = self.control_sender.send(ControlCommand::Subscribe(topic, tx)).await;
        rx.await.map_err(|_| SubscriptionError::PublishError(InsufficientPeers))?
    }

    pub async fn unsubscribe(&self, topic: TopicHash) {
        let _ = self.control_sender.unbounded_send(ControlCommand::Unsubscribed(topic));
    }

    pub fn heartbeat(&self) {
        let _ = self.control_sender.unbounded_send(Heartbeat);
    }

    /// List subscribed topics by name.
    pub async fn ls(&mut self) -> Result<Vec<TopicHash>, ()> {
        // let (tx, rx) = oneshot::channel();
        // self.control_sender.send(ControlCommand::Ls(tx)).await?;
        // Ok(rx.await?)

        unimplemented!()
    }

    // pub async fn get_peer_topic(&mut self, peer_id: PeerId) -> Result<HashMap<TopicHash, BTreeSet<PeerId>>, Box<dyn Error>> {
    //     let (tx, rx) = oneshot::channel();
    //     self.control_sender.send()
    // }

    /// List peers which we are currently exchanging only simple message.
    pub async fn dump_fanout_peer(&mut self) -> Result<HashMap<TopicHash, BTreeSet<PeerId>>, Box<dyn Error>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::GetFanoutPeer(tx)).await?;
        Ok(rx.await?)
    }

    /// List peers which we are currently exchanging full message.
    pub async fn dump_mesh_peer(&mut self) -> Result<HashMap<TopicHash, BTreeSet<PeerId>>, Box<dyn Error>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::GetMeshPeer(tx)).await?;
        Ok(rx.await?)
    }
}
