// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::PeerId;
use libp2p_traits::{ReadEx, WriteEx};
use crate::network::NetworkInfo;
use crate::{ProtocolId, SwarmError};
use crate::connection::ConnectionId;
use crate::substream::StreamId;
use crate::identify::IdentifyInfo;

type Result<T> = std::result::Result<T, SwarmError>;

#[derive(Debug)]
#[allow(dead_code)]
pub enum SwarmControlCmd<TSubstream> {
    /// Open a connection to the remote peer.
    NewConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Close any connection to the remote peer.
    CloseConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Open a new stream specified with protocol Ids to the remote peer.
    NewStream(PeerId, Vec<ProtocolId>, oneshot::Sender<Result<TSubstream>>),
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
/// The possible operations are implemented as async methods and redundantly
/// as poll-based variants which may be useful inside of other poll based
/// environments such as certain trait implementations.
//#[derive(Debug)]
pub struct Control<TSubstream> {
    /// Command channel to `Connection`.
    sender: mpsc::Sender<SwarmControlCmd<TSubstream>>,
}

impl<TSubstream> Clone for Control<TSubstream> {
    fn clone(&self) -> Self {
        Control {
            sender: self.sender.clone(),
        }
    }
}

impl<TSubstream> Control<TSubstream>
where
    TSubstream: ReadEx + WriteEx,
{
    pub(crate) fn new(sender: mpsc::Sender<SwarmControlCmd<TSubstream>>) -> Self {
        Control { sender }
    }

    /// make a connection to the remote.
    pub async fn new_connection(&mut self, peerd_id: PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SwarmControlCmd::NewConnection(peerd_id.clone(), tx)).await?;
        rx.await?
    }

    /// Open a new outbound stream towards the remote.
    pub async fn new_stream(&mut self, peerd_id: PeerId, pids: Vec<ProtocolId>) -> Result<TSubstream> {
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

    /// Close the connection.
    pub async fn close(&mut self) -> Result<()> {
        // SwarmControlCmd::CloseSwarm doesn't need a response from Swarm
        if self.sender.send(SwarmControlCmd::CloseSwarm).await.is_err() {
            // The receiver is closed which means the connection is already closed.
            return Ok(());
        }
        Ok(())
    }
}
