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

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2prs_core::peerstore::PeerStore;
use libp2prs_core::{Multiaddr, PeerId, ProtocolId, PublicKey};
use std::sync::Arc;
use std::time::Duration;

use crate::connection::{ConnectionId, ConnectionView};
use crate::identify::IdentifyInfo;
use crate::metrics::metric::Metric;
use crate::network::NetworkInfo;
use crate::substream::{StreamId, Substream, SubstreamView};
use crate::{SwarmError, SwarmStats, SWARM_EXIT_FLAG};
use std::collections::hash_map::IntoIter;
use std::sync::atomic::Ordering;

type Result<T> = std::result::Result<T, SwarmError>;

/// The control commands for [`Swarm`].
///
/// The `Swarm` controller manipulates the [`Swarm`] via these commands.
///
#[derive(Debug)]
pub enum SwarmControlCmd {
    /// Open a connection to the remote peer with address specified.
    Connect(PeerId, Vec<Multiaddr>, oneshot::Sender<Result<()>>),
    /// Open a connection to the remote peer. Parameter 'bool' means using Routing(if available) to
    /// lookup multiaddr of the remote peer.
    NewConnection(PeerId, bool, oneshot::Sender<Result<()>>),
    /// Close any connection to the remote peer.
    CloseConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Open a new stream specified with protocol Ids to the remote peer.
    /// Parameter 'bool' means using routing interface(if available) to
    /// lookup multiaddr of the remote peer.
    NewStream(PeerId, Vec<ProtocolId>, bool, oneshot::Sender<Result<Substream>>),
    /// Close a stream specified.
    CloseStream(ConnectionId, StreamId),
    /// Retrieve the self multi addresses of Swarm.
    SelfAddresses(oneshot::Sender<Vec<Multiaddr>>),
    /// Retrieve network information of Swarm.
    NetworkInfo(oneshot::Sender<NetworkInfo>),
    /// Retrieve network information of Swarm.
    IdentifyInfo(oneshot::Sender<IdentifyInfo>),
    ///
    Dump(DumpCommand),
}

/// The dump commands can be used to dump internal data of Swarm.
#[derive(Debug)]
pub enum DumpCommand {
    /// Dump the active connections belonged to some remote peer.
    /// None means dumping all active connections.
    Connections(Option<PeerId>, oneshot::Sender<Vec<ConnectionView>>),
    /// Dump all substreams of a connection.
    Streams(PeerId, oneshot::Sender<Result<Vec<SubstreamView>>>),
    /// Dump all statistics of Swarm.
    Statistics(oneshot::Sender<Result<SwarmStats>>),
}

/// The `Swarm` controller.
///
/// While a Yamux connection makes progress via its `next_stream` method,
/// this controller can be used to concurrently direct the connection,
/// e.g. to open a new stream to the remote or to close the connection.
///
#[derive(Clone)]
pub struct Control {
    /// Command channel to `Connection`.
    sender: mpsc::Sender<SwarmControlCmd>,
    /// PeerStore
    peer_store: PeerStore,
    /// Swarm metric
    metric: Arc<Metric>,
}

#[allow(dead_code)]
impl Control {
    pub(crate) fn new(sender: mpsc::Sender<SwarmControlCmd>, peer_store: PeerStore, metric: Arc<Metric>) -> Self {
        Control {
            sender,
            peer_store,
            metric,
        }
    }
    /// Return an iterator that contains all input bytes group by peer.
    pub fn peer_in_iter(&self) -> IntoIter<PeerId, usize> {
        self.metric.get_peers_in_list()
    }

    /// Return an iterator that contains all output bytes group by peer.
    pub fn peer_out_iter(&self) -> IntoIter<PeerId, usize> {
        self.metric.get_peers_out_list()
    }

    /// Return an iterator that contains all input bytes group by protocol.
    pub fn protocol_in_iter(&self) -> IntoIter<String, usize> {
        self.metric.get_protocols_in_list()
    }

    /// Return an iterator that contains all output bytes group by protocol.
    pub fn protocol_out_iter(&self) -> IntoIter<String, usize> {
        self.metric.get_protocols_out_list()
    }

    /// Get all peers in the AddrBook of Peerstore.
    pub fn get_peers(&self) -> Vec<PeerId> {
        self.peer_store.get_peers()
    }

    /// Get recv package count&bytes
    pub fn get_recv_count_and_size(&self) -> (usize, usize) {
        self.metric.get_recv_count_and_size()
    }

    /// Get send package count&bytes
    pub fn get_sent_count_and_size(&self) -> (usize, usize) {
        self.metric.get_sent_count_and_size()
    }

    /// Get recv&send bytes by protocol_id
    pub fn get_protocol_in_and_out(&self, protocol_id: &str) -> (Option<usize>, Option<usize>) {
        self.metric.get_protocol_in_and_out(protocol_id)
    }

    /// Get recv&send bytes by peer_id
    pub fn get_peer_in_and_out(&self, peer_id: &PeerId) -> (Option<usize>, Option<usize>) {
        self.metric.get_peer_in_and_out(peer_id)
    }

    /// Make a new connection towards the remote peer with addresses specified.
    pub async fn connect_with_addrs(&mut self, peer_id: PeerId, addrs: Vec<Multiaddr>) -> Result<()> {
        let (tx, rx) = oneshot::channel::<Result<()>>();
        self.sender.send(SwarmControlCmd::Connect(peer_id, addrs, tx)).await?;
        rx.await?
    }

    /// Make a new connection towards the remote peer.
    ///
    /// It will lookup the peer store for address of the peer, otherwise
    /// initiate the routing interface for querying the addresses, if routing
    /// is available.
    pub async fn new_connection(&mut self, peer_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel::<Result<()>>();
        self.sender.send(SwarmControlCmd::NewConnection(peer_id, true, tx)).await?;
        rx.await?
    }
    /// Make a new connection towards the remote peer, without using routing(Kad-DHT).
    pub async fn new_connection_no_routing(&mut self, peer_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel::<Result<()>>();
        self.sender.send(SwarmControlCmd::NewConnection(peer_id, false, tx)).await?;
        rx.await?
    }
    /// Close connection towards the remote peer.
    pub async fn disconnect(&mut self, peer_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::CloseConnection(peer_id, tx)).await?;
        rx.await?
    }

    /// Open a new outbound stream towards the remote peer.
    ///
    /// It will lookup the peer store for address of the peer,
    /// otherwise initiate the routing interface for address querying,
    /// when routing is enabled. In the end, it will open an outgoing
    /// sub-stream when the connection is eventually established.
    pub async fn new_stream(&mut self, peer_id: PeerId, pids: Vec<ProtocolId>) -> Result<Substream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewStream(peer_id, pids, true, tx)).await?;
        rx.await?
    }

    /// Open a new outbound stream towards the remote peer, without routing.
    pub async fn new_stream_no_routing(&mut self, peer_id: PeerId, pids: Vec<ProtocolId>) -> Result<Substream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewStream(peer_id, pids, false, tx)).await?;
        rx.await?
    }

    /// Retrieve the all listened addresses from Swarm.
    ///
    /// All listened addresses on interface and the observed addresses
    /// from Identify protocol.
    pub async fn self_addrs(&mut self) -> Result<Vec<Multiaddr>> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::SelfAddresses(tx)).await?;
        Ok(rx.await?)
    }

    /// Retrieve network information from Swarm.
    pub async fn retrieve_networkinfo(&mut self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NetworkInfo(tx)).await?;
        Ok(rx.await?)
    }

    /// Retrieve identify information from Swarm.
    pub async fn retrieve_identify_info(&mut self) -> Result<IdentifyInfo> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::IdentifyInfo(tx)).await?;
        Ok(rx.await?)
    }

    pub async fn dump_connections(&mut self, peer_id: Option<PeerId>) -> Result<Vec<ConnectionView>> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(SwarmControlCmd::Dump(DumpCommand::Connections(peer_id, tx)))
            .await?;
        Ok(rx.await?)
    }

    pub async fn dump_streams(&mut self, peer_id: PeerId) -> Result<Vec<SubstreamView>> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::Dump(DumpCommand::Streams(peer_id, tx))).await?;
        rx.await?
    }

    pub async fn dump_statistics(&mut self) -> Result<SwarmStats> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::Dump(DumpCommand::Statistics(tx))).await?;
        rx.await?
    }

    /// Close the swarm.
    pub fn close(&mut self) {
        // simply close the tx, then exit the main loop
        // TODO: wait for the main loop to exit before returning
        self.sender.close_channel();

        while !SWARM_EXIT_FLAG.load(Ordering::Relaxed) {}

        log::info!("!!!Swarm shutdown!!!")
    }

    /// Pins the peer Id so that GC wouldn't recycle the multiaddr of the peer.
    pub fn pin(&self, peer_id: &PeerId) {
        self.peer_store.pin(peer_id)
    }

    /// Unpins the peer Id.
    pub fn unpin(&self, peer_id: &PeerId) {
        self.peer_store.unpin(peer_id);
    }

    /// Checks if the peer is currently being pinned in peer store.
    pub fn pinned(&self, peer_id: &PeerId) -> bool {
        self.peer_store.pinned(peer_id)
    }

    /// Gets the public key by peer_id.
    pub fn get_key(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.peer_store.get_key(peer_id)
    }

    /// Gets all multiaddr of a peer.
    pub fn get_addrs(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        self.peer_store.get_addrs(peer_id)
    }

    /// Adds a address to address_book by peer_id, if exists, update rtt.
    pub fn add_addr(&self, peer_id: &PeerId, addr: Multiaddr, ttl: Duration) {
        self.peer_store.add_addr(peer_id, addr, ttl)
    }

    /// Adds many new addresses if they're not already in the peer store.
    pub fn add_addrs(&self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: Duration) {
        self.peer_store.add_addrs(peer_id, addrs, ttl)
    }

    /// Clears all multiaddr of a peer from the peer store.
    pub fn clear_addrs(&self, peer_id: &PeerId) {
        self.peer_store.clear_addrs(peer_id)
    }

    /// Updates ttl.
    pub fn update_addr(&self, peer_id: &PeerId, new_ttl: Duration) {
        self.peer_store.update_addr(peer_id, new_ttl)
    }

    /// Adds the protocols by peer_id.
    pub fn add_protocols(&self, peer_id: &PeerId, protos: Vec<String>) {
        self.peer_store.add_protocols(peer_id, protos);
    }

    /// Removes the protocols by peer_id.
    pub fn clear_protocols(&self, peer_id: &PeerId) {
        self.peer_store.clear_protocols(peer_id);
    }

    /// Gets the protocols supported by the specified PeerId.
    pub fn get_protocols(&self, peer_id: &PeerId) -> Option<Vec<String>> {
        self.peer_store.get_protocols(peer_id)
    }

    /// Tests if the PeerId supports the given protocols.
    pub fn first_supported_protocol(&self, peer_id: &PeerId, protos: Vec<String>) -> Option<String> {
        self.peer_store.first_supported_protocol(peer_id, protos)
    }

    /// Searches all protocols and return an option that matches by given protocols.
    fn support_protocols(&self, peer_id: &PeerId, protos: Vec<String>) -> Option<Vec<String>> {
        self.peer_store.support_protocols(peer_id, protos)
    }
}
