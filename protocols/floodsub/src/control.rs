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
use crate::subscription::Subscription;
use crate::Topic;
use crate::{FloodsubConfig, FloodsubError};
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2prs_core::PeerId;

pub(crate) enum ControlCommand {
    Publish(FloodsubMessage, oneshot::Sender<()>),
    Subscribe(Topic, oneshot::Sender<Subscription>),
    Ls(oneshot::Sender<Vec<Topic>>),
    GetPeers(Topic, oneshot::Sender<Vec<PeerId>>),
}

#[derive(Clone)]
pub struct Control {
    config: FloodsubConfig,
    control_sender: mpsc::UnboundedSender<ControlCommand>,
}

type Result<T> = std::result::Result<T, FloodsubError>;

impl Control {
    pub(crate) fn new(control_sender: mpsc::UnboundedSender<ControlCommand>, config: FloodsubConfig) -> Self {
        Control { control_sender, config }
    }
    /// Closes the floodsub main loop.
    pub fn close(&mut self) {
        self.control_sender.close_channel();
    }

    /// Publish publishes data to a given topic.
    pub async fn publish(&mut self, topic: Topic, data: impl Into<Vec<u8>>) -> Result<()> {
        let msg = FloodsubMessage {
            source: self.config.local_peer_id,
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could flood the network
            // with packets with the predetermined sequence numbers and absorb our legitimate
            // messages. We therefore use a random number.
            sequence_number: rand::random::<[u8; 20]>().to_vec(),
            topics: vec![topic.clone()],
        };

        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Publish(msg, tx)).await?;

        Ok(rx.await?)
    }

    /// Subscribe to messages on a given topic.
    pub async fn subscribe(&mut self, topic: Topic) -> Result<Subscription> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Subscribe(topic, tx)).await?;
        Ok(rx.await?)
    }

    /// List subscribed topics by name.
    pub async fn ls(&mut self) -> Result<Vec<Topic>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Ls(tx)).await?;
        Ok(rx.await?)
    }

    /// List peers we are currently pubsubbing with.
    pub async fn get_peers(&mut self, topic: Topic) -> Result<Vec<PeerId>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::GetPeers(topic, tx)).await?;
        Ok(rx.await?)
    }
}
