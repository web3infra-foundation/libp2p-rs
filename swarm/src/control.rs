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
use crate::network::NetworkInfo;
use crate::substream::{StreamId, Substream};
use crate::{ProtocolId, SwarmError};
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
}

impl Clone for Control {
    fn clone(&self) -> Self {
        Control {
            sender: self.sender.clone(),
        }
    }
}

impl Control {
    pub(crate) fn new(sender: mpsc::Sender<SwarmControlCmd>) -> Self {
        Control { sender }
    }

    /// make a connection to the remote.
    pub async fn new_connection(&mut self, peerd_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewConnection(peerd_id.clone(), tx)).await?;
        rx.await?
    }

    /// Open a new outbound stream towards the remote.
    pub async fn new_stream(&mut self, peerd_id: PeerId, pids: Vec<ProtocolId>) -> Result<Substream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewStream(peerd_id.clone(), pids, tx)).await?;
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
