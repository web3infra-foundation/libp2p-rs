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

use async_std::task;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    select,
};
use nohash_hasher::IntMap;

use crate::protocol::FloodsubMessage;
use crate::subscription::{SubId, Subscription};
use crate::{
    control::Control,
    protocol::{FloodsubRpc, FloodsubSubscription, FloodsubSubscriptionAction, Handler, RPC},
    FloodsubConfig, FloodsubError, Topic, FLOOD_SUB_ID,
};
use futures::stream::FusedStream;
use libp2prs_core::PeerId;
use libp2prs_swarm::substream::Substream;
use libp2prs_swarm::Control as Swarm_Control;
use libp2prs_traits::{ReadEx, WriteEx};
use std::collections::HashMap;

pub(crate) enum ControlCommand {
    Publish(FloodsubMessage, oneshot::Sender<()>),
    Subscribe(Topic, oneshot::Sender<Option<Subscription>>),
    Ls(oneshot::Sender<Vec<Topic>>),
    GetPeers(Topic, oneshot::Sender<Vec<PeerId>>),
}

pub(crate) enum PeerEvent {
    NewPeer(PeerId),
    DeadPeer(PeerId),
}

type Result<T> = std::result::Result<T, FloodsubError>;

pub struct FloodSub {
    config: FloodsubConfig,

    // Used to open stream.
    control: Option<Swarm_Control>,

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
    peers: HashMap<PeerId, mpsc::UnboundedSender<FloodsubRpc>>,

    // Topics tracks which topics each of our peers are subscribed to.
    topics: HashMap<Topic, HashMap<PeerId, ()>>,

    // The set of topics we are subscribed to.
    my_topics: HashMap<Topic, IntMap<SubId, mpsc::UnboundedSender<FloodsubMessage>>>,

    // Cancel subscription.
    cancel_tx: mpsc::UnboundedSender<Subscription>,
    cancel_rx: mpsc::UnboundedReceiver<Subscription>,
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
            control: None,
            peer_tx,
            peer_rx,
            incoming_tx,
            incoming_rx,
            control_tx,
            control_rx,
            peers: HashMap::default(),
            my_topics: HashMap::default(),
            topics: HashMap::default(),
            cancel_tx,
            cancel_rx,
        }
    }

    /// Get handler of floodsub, swarm will call "handle" func after muxer negotiate success.
    pub fn handler(&self) -> Handler {
        Handler::new(self.incoming_tx.clone(), self.peer_tx.clone())
    }

    /// Get control of floodsub, which can be used to publish or subscribe.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone(), self.config.clone())
    }

    /// Start message process loop.
    pub fn start(mut self, control: Swarm_Control) {
        self.control = Some(control);

        // well, self 'move' explicitly,
        let mut floodsub = self;
        task::spawn(async move {
            let _ = floodsub.process_loop().await;
        });
    }

    /// Message Process Loop.
    pub async fn process_loop(&mut self) -> Result<()> {
        let result = self.next().await;

        if !self.peer_rx.is_terminated() {
            self.peer_rx.close();
            while self.peer_rx.next().await.is_some() {
                // just drain
            }
        }

        if !self.incoming_rx.is_terminated() {
            self.incoming_rx.close();
            while self.incoming_rx.next().await.is_some() {
                // just drain
            }
        }

        if !self.control_rx.is_terminated() {
            self.control_rx.close();
            while let Some(cmd) = self.control_rx.next().await {
                match cmd {
                    ControlCommand::Publish(_, reply) => {
                        let _ = reply.send(());
                    }
                    ControlCommand::Subscribe(_, reply) => {
                        let _ = reply.send(None);
                    }
                    ControlCommand::Ls(reply) => {
                        let _ = reply.send(Vec::new());
                    }
                    ControlCommand::GetPeers(_, reply) => {
                        let _ = reply.send(Vec::new());
                    }
                }
            }
        }

        if !self.cancel_rx.is_terminated() {
            self.cancel_rx.close();
            while self.cancel_rx.next().await.is_some() {
                // just drain
            }
        }

        self.drop_all_peers();
        self.drop_all_my_topics();
        self.drop_all_topics();

        result
    }

    async fn next(&mut self) -> Result<()> {
        loop {
            select! {
                cmd = self.peer_rx.next() => {
                    self.handle_peer_event(cmd).await;
                }
                rpc = self.incoming_rx.next() => {
                    self.handle_incoming_rpc(rpc).await?;
                }
                cmd = self.control_rx.next() => {
                    self.on_control_command(cmd).await?;
                }
                sub = self.cancel_rx.next() => {
                    self.un_subscribe(sub).await?;
                }
            }
        }
    }

    // Tell new peer my subscribed topics.
    fn get_hello_packet(&self) -> FloodsubRpc {
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
        rpc
    }

    // Always wait to send message.
    async fn handle_sending_message(&mut self, rpid: PeerId, mut writer: Substream) {
        let (mut tx, mut rx) = mpsc::unbounded();

        let _ = tx.send(self.get_hello_packet()).await;

        self.peers.insert(rpid.clone(), tx);

        task::spawn(async move {
            loop {
                match rx.next().await {
                    Some(rpc) => {
                        log::trace!("send rpc msg: {:?}", rpc);
                        // if failed, should reset?
                        let _ = writer.write_one(rpc.into_bytes().as_slice()).await;
                    }
                    None => return,
                }
            }
        });
    }

    // If remote peer is dead, remove it from peers and topics.
    async fn handle_remove_dead_peer(&mut self, rpid: PeerId) {
        let tx = self.peers.remove(&rpid);
        match tx {
            Some(mut tx) => {
                let _ = tx.close().await;
            }
            None => return,
        }

        for ps in self.topics.values_mut() {
            ps.remove(&rpid);
        }
    }

    // Check if stream / connection is closed.
    async fn handle_peer_eof(&mut self, rpid: PeerId, mut reader: Substream) {
        let mut peer_dead_tx = self.peer_tx.clone();
        task::spawn(async move {
            loop {
                if reader.read_one(2048).await.is_err() {
                    let _ = peer_dead_tx.send(PeerEvent::DeadPeer(rpid.clone())).await;
                    return;
                }
            }
        });
    }

    // Handle when new peer connect.
    async fn handle_new_peer(&mut self, pid: PeerId) {
        let stream = self.control.as_mut().unwrap().new_stream(pid.clone(), vec![FLOOD_SUB_ID]).await;

        // if new stream failed, ignore it Since some peer don's support floodsub protocol
        if let Ok(stream) = stream {
            let writer = stream.clone();

            log::trace!("open stream to {}", pid);

            self.handle_sending_message(pid.clone(), writer).await;
            self.handle_peer_eof(pid.clone(), stream).await;
        }
    }

    // Handle peer event, include new peer event and peer dead event
    async fn handle_peer_event(&mut self, cmd: Option<PeerEvent>) {
        match cmd {
            Some(PeerEvent::NewPeer(rpid)) => {
                self.handle_new_peer(rpid).await;
            }
            Some(PeerEvent::DeadPeer(rpid)) => {
                self.handle_remove_dead_peer(rpid).await;
            }
            None => {}
        }
    }

    // Check if I subscribe these topics.
    fn subscribed_to_msg(&self, msg: FloodsubMessage) -> bool {
        if self.my_topics.is_empty() {
            return false;
        }

        for topic in msg.topics {
            if self.my_topics.contains_key(&topic) {
                return true;
            }
        }

        false
    }

    // Send message to all local subscribers of these topic.
    async fn notify_subs(&mut self, from: PeerId, msg: FloodsubMessage) -> Result<()> {
        if !self.config.subscribe_local_messages && self.config.local_peer_id == from {
            return Ok(());
        }

        for topic in &msg.topics {
            let subs = self.my_topics.get_mut(topic);
            if let Some(subs) = subs {
                for sender in subs.values_mut() {
                    sender.send(msg.clone()).await.or(Err(FloodsubError::Closed))?;
                }
            }
        }

        Ok(())
    }

    // Publish message to all remote subscriber of topics.
    async fn publish(&mut self, from: PeerId, msg: FloodsubMessage) -> Result<()> {
        let mut to_send = Vec::new();
        for topic in &msg.topics {
            let subs = self.topics.get(topic);
            if let Some(subs) = subs {
                for pid in subs.keys() {
                    to_send.push(pid.clone());
                }
            }
        }

        let rpc = FloodsubRpc {
            messages: vec![msg.clone()],
            subscriptions: vec![],
        };

        for pid in to_send {
            if pid == from || pid == msg.source {
                continue;
            }

            let sender = self.peers.get_mut(&pid);
            if let Some(sender) = sender {
                sender.send(rpc.clone()).await.or(Err(FloodsubError::Closed))?;
            }
        }

        Ok(())
    }

    // Publish message to all subscriber include local and remote.
    async fn publish_message(&mut self, from: PeerId, msg: FloodsubMessage) -> Result<()> {
        // TODO: reject unsigned messages when strict before we even process the id

        self.notify_subs(from.clone(), msg.clone()).await?;
        self.publish(from, msg).await
    }

    // Handle incoming rpc message received by Handler.
    async fn handle_incoming_rpc(&mut self, rpc: Option<RPC>) -> Result<()> {
        match rpc {
            Some(rpc) => {
                log::trace!("recv rpc {:?}", rpc);

                let from = rpc.from.clone();
                for sub in rpc.rpc.subscriptions {
                    match sub.action {
                        FloodsubSubscriptionAction::Subscribe => {
                            log::trace!("handle topic {:?}", sub.topic.clone());
                            let subs = self.topics.entry(sub.topic.clone()).or_insert_with(HashMap::<PeerId, ()>::default);
                            subs.insert(from.clone(), ());
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
                    if !self.subscribed_to_msg(msg.clone()) {
                        log::trace!("received message we didn't subscribe to. Dropping.");
                        continue;
                    }

                    self.publish_message(from.clone(), msg.clone()).await?;
                }

                Ok(())
            }
            None => Err(FloodsubError::Closed),
        }
    }

    // Announce my new subscribed topic to all connected peer.
    async fn announce(&mut self, topic: Topic, sub: FloodsubSubscriptionAction) -> Result<()> {
        let rpc = FloodsubRpc {
            messages: vec![],
            subscriptions: vec![FloodsubSubscription { action: sub, topic }],
        };

        for sender in self.peers.values_mut() {
            sender.send(rpc.clone()).await.or(Err(FloodsubError::Closed))?;
        }

        Ok(())
    }

    // Process publish or subscribe command.
    async fn on_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::Publish(msg, reply)) => {
                let lpid = self.config.local_peer_id.clone();
                self.publish_message(lpid, msg).await?;

                let _ = reply.send(());
            }
            Some(ControlCommand::Subscribe(topic, reply)) => {
                let sub = self.subscribe(topic).await?;
                let _ = reply.send(Some(sub));
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
                    for pid in self.peers.keys() {
                        peers.push(pid.clone());
                    }
                } else {
                    let subs = self.topics.get(&topic);
                    if let Some(subs) = subs {
                        for pid in subs.keys() {
                            peers.push(pid.clone());
                        }
                    }
                }
                let _ = reply.send(peers);
            }
            None => {}
        }

        Ok(())
    }

    // Subscribe one topic.
    async fn subscribe(&mut self, topic: Topic) -> Result<Subscription> {
        let subs = self
            .my_topics
            .entry(topic.clone())
            .or_insert_with(IntMap::<SubId, mpsc::UnboundedSender<FloodsubMessage>>::default);

        let mut announce = false;
        if subs.is_empty() {
            announce = true;
        }

        let sub_id = SubId::random();
        let (tx, rx) = mpsc::unbounded();
        subs.insert(sub_id, tx);

        if announce {
            self.announce(topic.clone(), FloodsubSubscriptionAction::Subscribe).await?;
        }

        Ok(Subscription::new(sub_id, topic, rx, self.cancel_tx.clone()))
    }

    // Unsubscribe one topic.
    async fn un_subscribe(&mut self, sub: Option<Subscription>) -> Result<()> {
        if let Some(sub) = sub {
            let topic = sub.topic.clone();
            let subs = self.my_topics.get_mut(&topic);
            let mut delete = false;
            let mut announce = false;
            if let Some(subs) = subs {
                if subs.remove(&sub.id).is_some() {
                    announce = true;
                }
                if subs.is_empty() {
                    delete = true;
                }
            }

            if delete {
                self.my_topics.remove(&topic);
            }

            if announce {
                self.announce(topic, FloodsubSubscriptionAction::Unsubscribe).await?;
            }
        }

        Ok(())
    }
}

impl FloodSub {
    fn drop_all_peers(&mut self) {
        for (p, tx) in self.peers.drain().take(1) {
            tx.close_channel();
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
