// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
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
    SinkExt,
};

use crate::{
    connection::{stream::Stream, ControlCommand},
    error::ConnectionError,
};

pub struct Control {
    sender: mpsc::UnboundedSender<ControlCommand>,
}

type Result<T> = std::result::Result<T, ConnectionError>;

impl Control {
    pub fn new(sender: mpsc::UnboundedSender<ControlCommand>) -> Self {
        Control { sender }
    }

    /// Open a new stream to the remote.
    pub async fn open_stream(&mut self) -> Result<Stream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(ControlCommand::OpenStream(tx)).await?;
        rx.await?
    }

    /// Accept a new stream from the remote.
    pub async fn accept_stream(&mut self) -> Result<Stream> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(ControlCommand::AcceptStream(tx)).await?;
        rx.await?
    }

    /// Close the connection.
    pub async fn close(&mut self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(ControlCommand::CloseConnection(tx)).await.is_err() {
            // The receiver is closed which means the connection is already closed.
            return Ok(());
        }
        // A dropped `oneshot::Sender` means the `Connection` is gone,
        // so we do not treat receive errors differently here.
        let _ = rx.await;
        Ok(())
    }
}

impl Clone for Control {
    fn clone(&self) -> Self {
        Control {
            sender: self.sender.clone(),
        }
    }
}
