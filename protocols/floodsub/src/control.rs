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

use crate::floodsub::ControlCommand;
use crate::protocol::FloodsubMessage;
use crate::subscription::Subscription;
use crate::FloodsubConfig;
use crate::Topic;
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2prs_core::PeerId;

#[derive(Clone)]
pub struct Control {
    config: FloodsubConfig,
    control_sender: mpsc::UnboundedSender<ControlCommand>,
}

impl Control {
    pub(crate) fn new(control_sender: mpsc::UnboundedSender<ControlCommand>, config: FloodsubConfig) -> Self {
        Control { control_sender, config }
    }

    /// Publish publishes data to a given topic.
    pub async fn publish(&mut self, topic: Topic, data: impl Into<Vec<u8>>) {
        let msg = FloodsubMessage {
            source: self.config.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could flood the network
            // with packets with the predetermined sequence numbers and absorb our legitimate
            // messages. We therefore use a random number.
            sequence_number: rand::random::<[u8; 20]>().to_vec(),
            topics: vec![topic.clone()],
        };

        let (tx, rx) = oneshot::channel();
        self.control_sender
            .send(ControlCommand::Publish(msg, tx))
            .await
            .expect("control send publish");
        rx.await.expect("publish");
    }

    /// Subscribe to messages on a given topic.
    pub async fn subscribe(&mut self, topic: Topic) -> Option<Subscription> {
        let (tx, rx) = oneshot::channel();
        self.control_sender
            .send(ControlCommand::Subscribe(topic, tx))
            .await
            .expect("control send subscribe");
        rx.await.expect("Subscribe")
    }

    /// List subscribed topics by name.
    pub async fn ls(&mut self) -> Vec<Topic> {
        let (tx, rx) = oneshot::channel();
        self.control_sender
            .send(ControlCommand::Ls(tx))
            .await
            .expect("control send subscribe");
        rx.await.expect("Subscribe")
    }

    /// List peers we are currently pubsubbing with.
    pub async fn get_peers(&mut self, topic: Topic) -> Vec<PeerId> {
        let (tx, rx) = oneshot::channel();
        self.control_sender
            .send(ControlCommand::GetPeers(topic, tx))
            .await
            .expect("control send subscribe");
        rx.await.expect("Subscribe")
    }
}
