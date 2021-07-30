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

use futures::{channel::mpsc, prelude::*, select};
use nohash_hasher::IntMap;

use crate::{
    control::{Control, ControlCommand},
    protocol::{FloodsubMessage, FloodsubRpc, FloodsubSubscription, FloodsubSubscriptionAction, Handler, PeerEvent, RPC},
    subscription::{SubId, Subscription},
    FloodsubConfig, FloodsubError, Topic, FLOOD_SUB_ID,
};
use futures::channel::mpsc::UnboundedReceiver;
use libp2prs_core::{PeerId, ProtocolId, WriteEx};
use libp2prs_runtime::task;
use libp2prs_swarm::protocol_handler::{IProtocolHandler, ProtocolImpl};
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::Control as SwarmControl;
use std::collections::HashMap;
use std::sync::Arc;

type Result<T> = std::result::Result<T, FloodsubError>;

#[allow(clippy::rc_buffer)]
pub struct FloodSub {
    config: FloodsubConfig,

    // Used to open stream.
    swarm: Option<SwarmControl>,

    // New peer is connected or peer is dead.
    peer_tx: mpsc::UnboundedSender<PeerEvent>,
    peer_rx: mpsc::UnboundedReceiver<PeerEvent>,

    // Used to recv incoming rpc message.
    incoming_tx: mpsc::UnboundedSender<RPC>,
    incoming_rx: mpsc::UnboundedReceiver<RPC>,

    // Used to pub/sub/ls/peers.
    control_tx: mpsc::UnboundedSender<ControlCommand>,
    control_rx: mpsc::UnboundedReceiver<ControlCommand>,

    // Connected peer.
    connected_peers: HashMap<PeerId, mpsc::UnboundedSender<Arc<Vec<u8>>>>,

    // Topics tracks which topics each of our peers are subscribed to.
    topics: HashMap<Topic, HashMap<PeerId, ()>>,

    // The set of topics we are subscribed to.
    my_topics: HashMap<Topic, IntMap<SubId, mpsc::UnboundedSender<Arc<FloodsubMessage>>>>,

    // Cancel subscription.
    cancel_tx: mpsc::UnboundedSender<(Topic, SubId)>,
    cancel_rx: mpsc::UnboundedReceiver<(Topic, SubId)>,
}

impl FloodSub {
    /// Create a new 'FloodSub'.
    pub fn new(config: FloodsubConfig) -> Self {
        let (peer_tx, peer_rx) = mpsc::unbounded();
        let (incoming_tx, incoming_rx) = mpsc::unbounded();
        let (control_tx, control_rx) = mpsc::unbounded();
        let (cancel_tx, cancel_rx) = mpsc::unbounded();
        FloodSub {
            config,
            swarm: None,
            peer_tx,
            peer_rx,
            incoming_tx,
            incoming_rx,
            control_tx,
            control_rx,
            connected_peers: HashMap::default(),
            my_topics: HashMap::default(),
            topics: HashMap::default(),
            cancel_tx,
            cancel_rx,
        }
    }

    /// Get control of floodsub, which can be used to publish or subscribe.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone(), self.config.clone())
    }

    /// Message Process Loop.
    async fn process_loop(&mut self) -> Result<()> {
        let result = self.next().await;

        self.drop_all_peers();
        self.drop_all_topics();
        self.drop_all_my_topics();

        result
    }

    async fn next(&mut self) -> Result<()> {
        loop {
            select! {
                cmd = self.peer_rx.next() => {
                    self.handle_peer_event(cmd);
                }
                rpc = self.incoming_rx.next() => {
                    self.handle_incoming_rpc(rpc);
                }
                cmd = self.control_rx.next() => {
                    self.on_control_command(cmd)?;
                }
                sub = self.cancel_rx.next() => {
                    self.un_subscribe(sub);
                }
            }
        }
    }

    // Tell new peer my subscribed topics.
    #[allow(clippy::rc_buffer)]
    fn get_hello_packet(&self) -> Arc<Vec<u8>> {
        // We need to send our subscriptions to the newly-connected node.
        let mut rpc = FloodsubRpc {
            messages: vec![],
            subscriptions: vec![],
        };
        for topic in self.my_topics.keys() {
            let subscription = FloodsubSubscription {
                action: FloodsubSubscriptionAction::Subscribe,
                topic: topic.clone(),
            };
            rpc.subscriptions.push(subscription);
        }
        Arc::new(rpc.into_bytes())
    }

    // Always wait to send message.
    fn handle_new_peer(&mut self, rpid: PeerId) {
        let mut swarm = self.swarm.clone().expect("swarm??");
        let peer_dead_tx = self.peer_tx.clone();
        let (tx, rx) = mpsc::unbounded();

        let _ = tx.unbounded_send(self.get_hello_packet());

        self.connected_peers.insert(rpid, tx);

        task::spawn(async move {
            let stream = swarm.new_stream(rpid, vec![ProtocolId::new(FLOOD_SUB_ID, 0)]).await;

            match stream {
                Ok(stream) => {
                    if handle_send_message(rx, stream).await.is_err() {
                        // write failed
                        let _ = peer_dead_tx.unbounded_send(PeerEvent::DeadPeer(rpid));
                    }
                }
                Err(_) => {
                    // new stream failed
                    let _ = peer_dead_tx.unbounded_send(PeerEvent::DeadPeer(rpid));
                }
            }
        });
    }

    // If remote peer is dead, remove it from peers and topics.
    fn handle_remove_dead_peer(&mut self, rpid: PeerId) {
        self.connected_peers.remove(&rpid);
        for ps in self.topics.values_mut() {
            ps.remove(&rpid);
        }
    }

    // Handle peer event, include new peer event and peer dead event
    fn handle_peer_event(&mut self, cmd: Option<PeerEvent>) {
        match cmd {
            Some(PeerEvent::NewPeer(rpid)) => {
                log::trace!("new peer {} has connected", rpid);
                self.handle_new_peer(rpid);
            }
            Some(PeerEvent::DeadPeer(rpid)) => {
                log::trace!("peer {} has disconnected", rpid);
                self.handle_remove_dead_peer(rpid);
            }
            None => {
                unreachable!()
            }
        }
    }

    // Check if I subscribe these topics.
    fn subscribed_to_msg(&self, topics: &[Topic]) -> bool {
        if self.my_topics.is_empty() {
            return false;
        }

        for topic in topics {
            if self.my_topics.contains_key(topic) {
                return true;
            }
        }

        false
    }

    // Send message to all local subscribers of these topic.
    fn notify_subs(&mut self, from: PeerId, msg: FloodsubMessage) {
        if !self.config.subscribe_local_messages && self.config.local_peer_id == from {
            return;
        }

        let msg = Arc::new(msg);
        for topic in &msg.topics {
            let _ = self.my_topics.get(topic).map(|subs| {
                subs.values().for_each(|sender| {
                    // TODO: remove sender if send failed
                    let _ = sender.unbounded_send(msg.clone());
                })
            });
        }
    }

    // Publish message to all remote subscriber of topics.
    fn publish(&mut self, from: PeerId, msg: FloodsubMessage) {
        let source = msg.source;
        let mut to_send = Vec::new();
        for topic in &msg.topics {
            let subs = self.topics.get(topic);
            if let Some(subs) = subs {
                for pid in subs.keys() {
                    to_send.push(pid);
                }
            }
        }

        let rpc = Arc::new(
            FloodsubRpc {
                messages: vec![msg],
                subscriptions: vec![],
            }
            .into_bytes(),
        );

        for pid in to_send {
            if *pid == from || *pid == source {
                continue;
            }

            self.connected_peers.get(&pid).map(|tx| tx.unbounded_send(rpc.clone()));
        }
    }

    // Publish message to all subscriber include local and remote.
    fn publish_message(&mut self, from: PeerId, msg: FloodsubMessage) {
        // TODO: reject unsigned messages when strict before we even process the id

        self.notify_subs(from, msg.clone());
        self.publish(from, msg);
    }

    // Handle incoming rpc message received by Handler.
    fn handle_incoming_rpc(&mut self, rpc: Option<RPC>) {
        match rpc {
            Some(rpc) => {
                log::trace!("recv rpc {:?}", rpc);

                let from = rpc.from;
                for sub in rpc.rpc.subscriptions {
                    match sub.action {
                        FloodsubSubscriptionAction::Subscribe => {
                            log::trace!("handle topic {:?}", sub.topic.clone());
                            let subs = self.topics.entry(sub.topic.clone()).or_insert_with(HashMap::<PeerId, ()>::default);
                            subs.insert(from, ());
                        }
                        FloodsubSubscriptionAction::Unsubscribe => {
                            let subs = self.topics.get_mut(&sub.topic.clone());
                            if let Some(subs) = subs {
                                subs.remove(&from);
                            }
                        }
                    }
                }

                for msg in rpc.rpc.messages {
                    if !self.subscribed_to_msg(&msg.topics) {
                        log::trace!("received message we didn't subscribe to. Dropping.");
                        continue;
                    }

                    self.publish_message(from, msg);
                }
            }
            None => {
                unreachable!()
            }
        }
    }

    // Announce my new subscribed topic to all connected peer.
    fn announce(&mut self, topic: Topic, sub: FloodsubSubscriptionAction) {
        let rpc = Arc::new(
            FloodsubRpc {
                messages: vec![],
                subscriptions: vec![FloodsubSubscription { action: sub, topic }],
            }
            .into_bytes(),
        );

        for tx in self.connected_peers.values() {
            let _ = tx.unbounded_send(rpc.clone());
        }
    }

    // Process publish or subscribe command.
    fn on_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::Publish(msg, reply)) => {
                let lpid = self.config.local_peer_id;
                self.publish_message(lpid, msg);

                let _ = reply.send(());
            }
            Some(ControlCommand::Subscribe(topic, reply)) => {
                let sub = self.subscribe(topic);
                let _ = reply.send(sub);
            }
            Some(ControlCommand::Ls(reply)) => {
                let mut topics = Vec::new();
                for t in self.my_topics.keys() {
                    topics.push(t.clone());
                }
                let _ = reply.send(topics);
            }
            Some(ControlCommand::GetPeers(topic, reply)) => {
                let mut peers = Vec::new();
                if topic.is_empty() {
                    for pid in self.connected_peers.keys() {
                        peers.push(*pid);
                    }
                } else {
                    let subs = self.topics.get(&topic);
                    if let Some(subs) = subs {
                        for pid in subs.keys() {
                            peers.push(*pid);
                        }
                    }
                }
                let _ = reply.send(peers);
            }
            None => return Err(FloodsubError::Closed),
        }

        Ok(())
    }

    // Subscribe one topic.
    fn subscribe(&mut self, topic: Topic) -> Subscription {
        let mut subs = self.my_topics.remove(&topic).unwrap_or_default();
        if subs.is_empty() {
            self.announce(topic.clone(), FloodsubSubscriptionAction::Subscribe);
        }

        let sub_id = SubId::random();
        let (tx, rx) = mpsc::unbounded();
        subs.insert(sub_id, tx);
        self.my_topics.insert(topic.clone(), subs);

        Subscription::new(sub_id, topic, rx, self.cancel_tx.clone())
    }

    // Unsubscribe one topic.
    fn un_subscribe(&mut self, sub: Option<(Topic, SubId)>) {
        match sub {
            Some((topic, id)) => {
                let subs = self.my_topics.remove(&topic);
                if let Some(mut subs) = subs {
                    if subs.remove(&id).is_some() {
                        self.announce(topic.clone(), FloodsubSubscriptionAction::Unsubscribe);
                    }

                    if !subs.is_empty() {
                        self.my_topics.insert(topic, subs);
                    }
                }
            }
            None => {
                unreachable!()
            }
        }
    }
}

impl FloodSub {
    fn drop_all_peers(&mut self) {
        for (p, _tx) in self.connected_peers.drain().take(1) {
            log::trace!("drop peer {}", p);
        }
    }

    fn drop_all_my_topics(&mut self) {
        for (t, mut subs) in self.my_topics.drain().take(1) {
            for (id, tx) in subs.drain().take(1) {
                tx.close_channel();
                log::trace!("drop peer {} in myTopic {:?}", id, t.clone());
            }
        }
    }

    fn drop_all_topics(&mut self) {
        for (t, mut subs) in self.topics.drain().take(1) {
            for (p, _) in subs.drain().take(1) {
                log::trace!("drop peer {} in topic {:?}", p, t.clone());
            }
        }
    }
}

impl ProtocolImpl for FloodSub {
    fn handlers(&self) -> Vec<IProtocolHandler> {
        vec![Box::new(Handler::new(self.incoming_tx.clone(), self.peer_tx.clone()))]
    }

    fn start(mut self, swarm: SwarmControl) -> Option<task::TaskHandle<()>> {
        self.swarm = Some(swarm);

        // well, self 'move' explicitly,
        let mut floodsub = self;
        let task = task::spawn(async move {
            let _ = floodsub.process_loop().await;
        });

        Some(task)
    }
}

#[allow(clippy::rc_buffer)]
async fn handle_send_message(mut rx: UnboundedReceiver<Arc<Vec<u8>>>, mut writer: Substream) -> Result<()> {
    loop {
        match rx.next().await {
            Some(rpc) => {
                log::trace!("send rpc msg: {:?}", rpc);
                writer.write_one(rpc.as_slice()).await?
            }
            None => {
                log::trace!("peer had been removed from floodsub");
                return Ok(());
            }
        }
    }
}
