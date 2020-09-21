// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use crate::SwarmError;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::PeerId;

type Result<T> = std::result::Result<T, SwarmError>;

#[derive(Debug)]
pub enum SwarmControlCmd<TSubstream> {
    /// Open a connection to the remote peer.
    NewConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Close any connection to the remote peer.
    CloseConnection(PeerId, oneshot::Sender<Result<()>>),
    /// Open a new stream to the remote peer.
    NewStream(PeerId, oneshot::Sender<Result<TSubstream>>),
    /// Close the whole connection.
    CloseSwarm,
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

impl<TSubstream> Control<TSubstream> {
    pub(crate) fn new(sender: mpsc::Sender<SwarmControlCmd<TSubstream>>) -> Self {
        Control { sender }
    }

    /// make a connection to the remote.
    pub async fn new_connection(&mut self, peerd_id: &PeerId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(SwarmControlCmd::NewConnection(peerd_id.clone(), tx))
            .await?;
        rx.await?
    }

    /// Open a new stream to the remote.
    pub async fn new_stream(&mut self, peerd_id: &PeerId) -> Result<TSubstream> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(SwarmControlCmd::NewStream(peerd_id.clone(), tx))
            .await?;
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
