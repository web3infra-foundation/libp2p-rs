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
use libp2prs_core::PeerId;

use crate::connection::ConnectionId;
use crate::identify::IdentifyInfo;
use crate::metrics::metric::Metric;
use crate::network::NetworkInfo;
use crate::substream::{StreamId, Substream};
use crate::{ProtocolId, SwarmError};
use std::sync::Arc;
use std::time::Duration;

type Result<T> = std::result::Result<T, SwarmError>;

/// The control commands for [`Swarm`].
///
/// The `Swarm` controller manipulates the [`Swarm`] via these commands.
///
#[derive(Debug)]
#[allow(dead_code)]
pub enum SwarmControlCmd {
    /// Open a connection to the remote peer.
    NewConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Close any connection to the remote peer.
    CloseConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Open a new stream specified with protocol Ids to the remote peer.
    NewStream(PeerId, Vec<ProtocolId>, oneshot::Sender<Result<Substream>>),
    /// Close a stream specified.
    CloseStream(ConnectionId, StreamId),
    /// Close the whole connection.
    CloseSwarm,
    /// Retrieve network information of Swarm
    NetworkInfo(oneshot::Sender<Result<NetworkInfo>>),
    /// Retrieve network information of Swarm
    IdentifyInfo(oneshot::Sender<Result<IdentifyInfo>>),
}

/// The `Swarm` controller.
///
/// While a Yamux connection makes progress via its `next_stream` method,
/// this controller can be used to concurrently direct the connection,
/// e.g. to open a new stream to the remote or to close the connection.
///
//#[derive(Debug)]
pub struct Control {
    /// Command channel to `Connection`.
    sender: mpsc::Sender<SwarmControlCmd>,
    /// Swarm metric
    metric: Arc<Metric>,
}

impl Clone for Control {
    fn clone(&self) -> Self {
        Control {
            sender: self.sender.clone(),
            metric: self.metric.clone(),
        }
    }
}

impl Control {
    pub(crate) fn new(sender: mpsc::Sender<SwarmControlCmd>, metric: Arc<Metric>) -> Self {
        Control { sender, metric }
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
    pub fn get_protocol_in_and_out(&self, protocol_id: &ProtocolId) -> (Option<usize>, Option<usize>) {
        self.metric.get_protocol_in_and_out(protocol_id)
    }

    /// Get recv&send bytes by peer_id
    pub fn get_peer_in_and_out(&self, peer_id: &PeerId) -> (Option<usize>, Option<usize>) {
        self.metric.get_peer_in_and_out(peer_id)
    }

    /// Make a new connection towards the remote peer.
    pub async fn new_connection(&mut self, peer_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel::<Result<()>>();
        let _ = self.sender.send(SwarmControlCmd::NewConnection(peer_id.clone(), tx)).await;
        rx.await?
    }

    /// Open a new outbound stream towards the remote peer.
    pub async fn new_stream(&mut self, peer_id: PeerId, pids: Vec<ProtocolId>) -> Result<Substream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewStream(peer_id, pids, tx)).await?;
        rx.await?
    }

    /// Retrieve network statistics from Swarm.
    pub async fn retrieve_networkinfo(&mut self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NetworkInfo(tx)).await?;
        rx.await?
    }

    /// Close the swarm.
    pub async fn close(&mut self) -> Result<()> {
        // SwarmControlCmd::CloseSwarm doesn't need a response from Swarm
        if self.sender.send(SwarmControlCmd::CloseSwarm).await.is_err() {
            // The receiver is closed which means the connection is already closed.
            return Ok(());
        }
        self.sender.close_channel();
        std::thread::sleep(Duration::from_secs(5));
        log::info!("Exit success");
        Ok(())
    }
}
