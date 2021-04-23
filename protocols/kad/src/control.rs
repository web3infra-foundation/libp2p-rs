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

use futures::channel::{mpsc, oneshot};
use futures::SinkExt;

use async_trait::async_trait;

use libp2prs_core::routing::{IRouting, Routing};
use libp2prs_core::transport::TransportError;
use libp2prs_core::{Multiaddr, PeerId};

use crate::kad::{KBucketView, KademliaStats, StorageStats};
use crate::protocol::{KadMessengerView, KadPeer};
use crate::query::PeerRecord;
use crate::{record, KadError};

type Result<T> = std::result::Result<T, KadError>;

pub(crate) enum ControlCommand {
    /// Initiates bootstrapping to join the Kad DHT network.
    ///
    /// The optional channel could be used to return the result of bootstrapping.
    Bootstrap(Vec<(PeerId, Multiaddr)>, Option<oneshot::Sender<Result<()>>>),
    /// Lookups the closer peers with given ID, returns a list of peer Id.
    Lookup(record::Key, oneshot::Sender<Result<Vec<KadPeer>>>),
    /// Searches for a peer with given ID, returns a list of peer info
    /// with relevant addresses.
    FindPeer(PeerId, oneshot::Sender<Result<KadPeer>>),
    /// Lookups peers who are able to provide a given key.
    FindProviders(record::Key, usize, oneshot::Sender<Result<Vec<KadPeer>>>),
    /// Adds the given key to the content routing system.
    ///
    /// It also announces it, otherwise it is just kept in the local
    /// accounting of which objects are being provided.
    Providing(record::Key, oneshot::Sender<Result<()>>),
    /// Removes the give key from the local provider store.
    ///
    /// This is a local operation. The local node will still be considered as a
    /// provider for the key by other nodes until these provider records expire.
    Unprovide(record::Key),
    /// Adds value corresponding to given Key.
    PutValue(record::Key, Vec<u8>, oneshot::Sender<Result<()>>),
    /// Searches value corresponding to given Key.
    GetValue(record::Key, oneshot::Sender<Result<PeerRecord>>),
    /// Dumps commands for debugging purpose.
    Dump(DumpCommand),
    /// Adds a peer node to Kad KBuckets, and its multiaddr to Peerstore.
    AddNode(PeerId, Vec<Multiaddr>),
    /// Removes a peer from Kad KBuckets, also removes it from Peerstore.
    RemoveNode(PeerId),
}

/// The dump commands can be used to dump internal data of Kad-DHT.
#[derive(Debug)]
pub enum DumpCommand {
    /// Dump the Providers in the local storage.
    Storage(oneshot::Sender<StorageStats>),
    /// Dump the Kad DHT kbuckets. Empty kbucket will be ignored.
    Entries(oneshot::Sender<Vec<KBucketView>>),
    /// Dump the Kad statistics.
    Statistics(oneshot::Sender<KademliaStats>),
    /// Dump the Kad Messengers.
    Messengers(oneshot::Sender<Vec<KadMessengerView>>),
}

#[derive(Clone)]
pub struct Control {
    control_sender: mpsc::UnboundedSender<ControlCommand>,
}

impl Control {
    pub(crate) fn new(control_sender: mpsc::UnboundedSender<ControlCommand>) -> Self {
        Control { control_sender }
    }

    /// Closes the Kad main loop and tasks.
    pub fn close(&mut self) {
        self.control_sender.close_channel();
    }

    /// Add a node and its listening addresses to KBuckets.
    pub async fn add_node(&mut self, peer_id: PeerId, addrs: Vec<Multiaddr>) {
        let _ = self.control_sender.send(ControlCommand::AddNode(peer_id, addrs)).await;
    }

    /// Add a node and its listening addresses to KBuckets.
    pub async fn remove_node(&mut self, peer_id: PeerId) {
        let _ = self.control_sender.send(ControlCommand::RemoveNode(peer_id)).await;
    }

    pub async fn dump_storage(&mut self) -> Result<StorageStats> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Dump(DumpCommand::Storage(tx))).await?;
        Ok(rx.await?)
    }

    pub async fn dump_kbuckets(&mut self) -> Result<Vec<KBucketView>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Dump(DumpCommand::Entries(tx))).await?;
        Ok(rx.await?)
    }

    pub async fn dump_statistics(&mut self) -> Result<KademliaStats> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Dump(DumpCommand::Statistics(tx))).await?;
        Ok(rx.await?)
    }

    pub async fn dump_messengers(&mut self) -> Result<Vec<KadMessengerView>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Dump(DumpCommand::Messengers(tx))).await?;
        Ok(rx.await?)
    }

    /// Initiates bootstrapping.
    ///
    /// Most likely it should be invoked once upon Kad startup.
    pub async fn bootstrap(&mut self, boot: Vec<(PeerId, Multiaddr)>) {
        let _ = self.control_sender.send(ControlCommand::Bootstrap(boot, None)).await;
    }

    /// Initiates bootstrapping and waits for the result.
    pub async fn bootstrap_wait(&mut self, boot: Vec<(PeerId, Multiaddr)>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let _ = self.control_sender.send(ControlCommand::Bootstrap(boot, Some(tx))).await;
        rx.await?
    }

    /// Lookup the closer peers with the given key.
    pub async fn lookup(&mut self, key: record::Key) -> Result<Vec<KadPeer>> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::Lookup(key, tx)).await?;
        rx.await?
    }

    pub async fn find_peer(&mut self, peer_id: &PeerId) -> Result<KadPeer> {
        let (tx, rx) = oneshot::channel();
        self.control_sender.send(ControlCommand::FindPeer(*peer_id, tx)).await?;
        rx.await?
    }

    pub async fn put_value(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let key = record::Key::from(key);
        self.control_sender.send(ControlCommand::PutValue(key, value, tx)).await?;
        rx.await?
    }

    pub async fn get_value(&mut self, key: Vec<u8>) -> Result<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        let key = record::Key::from(key);
        self.control_sender.send(ControlCommand::GetValue(key, tx)).await?;
        rx.await?.map(|t| t.record.value)
    }

    pub async fn provide(&mut self, key: Vec<u8>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let key = record::Key::from(key);
        self.control_sender.send(ControlCommand::Providing(key, tx)).await?;
        rx.await?
    }

    pub async fn unprovide(&mut self, key: Vec<u8>) -> Result<()> {
        let key = record::Key::from(key);
        self.control_sender.send(ControlCommand::Unprovide(key)).await?;
        Ok(())
    }

    pub async fn find_providers(&mut self, key: Vec<u8>, count: usize) -> Result<Vec<KadPeer>> {
        let (tx, rx) = oneshot::channel();
        let key = record::Key::from(key);
        self.control_sender.send(ControlCommand::FindProviders(key, count, tx)).await?;
        rx.await?
    }
}

/// Implements `Routing` for Kad Control. Therefore, Kad control can be used
/// by Swarm to find peers.
#[async_trait]
impl Routing for Control {
    async fn find_peer(&mut self, peer_id: &PeerId) -> std::result::Result<Vec<Multiaddr>, TransportError> {
        let kad_peer = self.find_peer(peer_id).await.map_err(|e| TransportError::Routing(e.into()))?;
        Ok(kad_peer.multiaddrs)
    }

    async fn find_providers(&mut self, key: Vec<u8>, count: usize) -> std::result::Result<Vec<PeerId>, TransportError> {
        let addrs = self
            .find_providers(key, count)
            .await
            .map_err(|e| TransportError::Routing(e.into()))?;
        Ok(addrs.into_iter().map(|peer| peer.node_id).collect())
    }

    async fn provide(&mut self, key: Vec<u8>) -> std::result::Result<(), TransportError> {
        let _ = self.provide(key).await.map_err(|e| TransportError::Routing(e.into()))?;
        Ok(())
    }

    fn box_clone(&self) -> IRouting {
        Box::new(self.clone())
    }
}
