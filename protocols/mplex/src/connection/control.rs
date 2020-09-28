use futures::{
    channel::{mpsc, oneshot},
    SinkExt,
};

use crate::{
    connection::{stream::Stream, ControlCommand},
    error::ConnectionError,
};

pub struct Control {
    sender: mpsc::Sender<ControlCommand>,
}

type Result<T> = std::result::Result<T, ConnectionError>;

impl Control {
    pub fn new(sender: mpsc::Sender<ControlCommand>) -> Self {
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
