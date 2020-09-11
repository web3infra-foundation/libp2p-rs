pub mod control;
pub mod stream;

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    select,
    stream::FusedStream,
};

use crate::{
    error::ConnectionError,
    frame::{io, Frame, FrameDecodeError, StreamID, Tag},
    pause::Pausable,
};
use control::Control;
use libp2p_traits::{ext::split::{ReadHalf, WriteHalf}, Read2, Write2, ReadExt2};
use nohash_hasher::IntMap;
use std::fmt;
use stream::Stream;

/// `Control` to `Connection` commands.
#[derive(Debug)]
pub enum ControlCommand {
    /// Open a new stream to the remote end.
    OpenStream(oneshot::Sender<Result<Stream>>),
    /// Open a new stream to the remote end.
    AcceptStream(oneshot::Sender<Result<Stream>>),
    /// Close the whole connection.
    CloseConnection(oneshot::Sender<()>),
}

/// `Stream` to `Connection` commands.
#[derive(Debug)]
pub(crate) enum StreamCommand {
    /// A new frame should be sent to the remote.
    SendFrame(Frame),
    /// Close a stream.
    CloseStream(StreamID),
}

/// The connection identifier.
///
/// Randomly generated, this is mainly intended to improve log output.
#[derive(Clone, Copy)]
pub(crate) struct Id(u32);

impl Id {
    /// Create a random connection ID.
    pub(crate) fn random() -> Self {
        Id(rand::random())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// This enum captures the various stages of shutting down the connection.
#[derive(Debug)]
enum Shutdown {
    /// We are open for business.
    NotStarted,
    /// We have received a `ControlCommand::Close` and are shutting
    /// down operations. The `Sender` will be informed once we are done.
    InProgress(oneshot::Sender<()>),
    /// The shutdown is complete and we are closed for good.
    Complete,
}

impl Shutdown {
    fn has_not_started(&self) -> bool {
        if let Shutdown::NotStarted = self {
            true
        } else {
            false
        }
    }

    fn is_in_progress(&self) -> bool {
        if let Shutdown::InProgress(_) = self {
            true
        } else {
            false
        }
    }

    fn is_complete(&self) -> bool {
        if let Shutdown::Complete = self {
            true
        } else {
            false
        }
    }
}

/// Arbitrary limit of our internal command channels.
///
/// Since each `mpsc::Sender` gets a guaranteed slot in a channel the
/// actual upper bound is this value + number of clones.
const MAX_COMMAND_BACKLOG: usize = 32;

type Result<T> = std::result::Result<T, ConnectionError>;

pub struct Connection<T> {
    id: Id,
    reader: Option<io::IO<ReadHalf<T>>>,
    socket: io::IO<WriteHalf<T>>,
    is_closed: bool,
    shutdown: Shutdown,
    next_stream_id: u32,
    streams: IntMap<StreamID, mpsc::Sender<Vec<u8>>>,
    stream_sender: mpsc::Sender<StreamCommand>,
    stream_receiver: mpsc::Receiver<StreamCommand>,
    control_sender: mpsc::Sender<ControlCommand>,
    control_receiver: Pausable<mpsc::Receiver<ControlCommand>>,
    accept_stream:  Option<oneshot::Sender<Result<stream::Stream>>>,
}

impl<T: Read2 + Write2 + Unpin + Send> Connection<T> {
    pub fn new(socket: T) -> Self {
        let id = Id::random();
        log::debug!("new connection: {}", id);

        let (reader, writer) = socket.split();
        let reader = Option::Some(io::IO::new(id, reader));


        let socket = io::IO::new(id, writer);
        let (stream_sender, stream_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
        let (control_sender, control_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);

        Connection {
            id,
            reader,
            socket,
            is_closed: false,
            next_stream_id: 0,
            shutdown: Shutdown::NotStarted,
            streams: IntMap::default(),
            stream_sender,
            stream_receiver,
            control_sender,
            control_receiver: Pausable::new(control_receiver),
            accept_stream:  None
        }
    }

    // The param of control must be
    pub fn control(&self) -> Result<Control> {
        let ctrl = Control::new(self.control_sender.clone());
        Ok(ctrl)
    }

    pub async fn close(&mut self) {
        self.socket.close().await;
    }

    pub async fn next_stream(&mut self) -> Result<()> {
        if self.is_closed {
            log::debug!("{}: connection is closed", self.id);
            return Ok(());
        }

        let result = self.handle_coming().await;
        // if let Ok(Some(_)) = result {
        //     return result;
        // }

        self.is_closed = true;

        // Close and drain the control command receiver.
        if !self.control_receiver.stream().is_terminated() {
            if self.control_receiver.is_paused() {
                self.control_receiver.unpause();
            }
            self.control_receiver.stream().close();

            while let Some(cmd) = self.control_receiver.next().await {
                match cmd {
                    ControlCommand::OpenStream(reply) => {
                        let _ = reply.send(Err(ConnectionError::Closed));
                    }
                    ControlCommand::AcceptStream(reply) => {
                        let _ = reply.send(Err(ConnectionError::Closed));
                    }
                    ControlCommand::CloseConnection(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        }

        self.drop_all_streams().await;
        // Close and drain the stream command receiver.
        if !self.stream_receiver.is_terminated() {
            self.stream_receiver.close();
            while let Some(_) = self.stream_receiver.next().await {
                // drop it
                log::info!("drop stream receiver frame");
            }
        }

        if let Err(ConnectionError::Closed) = result {
            return Ok(());
        }

        result
    }

    pub async fn handle_coming(&mut self) -> Result<()> {
        if let Some(reader) = self.reader.take() {
            let mut reader = futures::stream::unfold(reader, |mut io| async {
                Some((io.recv_frame().await, io))
            }.boxed());


            loop {
                select! {
                    // handle incoming
                    // frame = self.socket.recv_frame().fuse() => {
                    frame = reader.next() => {
                        if let Some(f) = frame {
                            let frame = f?;
                            self.on_frame(frame).await?;
                        }
                        // let frame = frame?;
                        // if let Some(stream) = self.on_frame(frame).await? {
                        //     return Ok(Some(stream));
                        // }
                    }
                    // handle outcoming
                    scmd = self.stream_receiver.next() => {
                        self.on_stream_command(scmd).await?;
                    }
                    ccmd = self.control_receiver.next() => {
                        self.on_control_command(ccmd).await?;
                    }
                }
            }
        }

        Err(ConnectionError::Closed)
    }

    async fn on_frame(&mut self, frame: Frame) -> Result<()> {
        log::trace!("{}: received: {}", self.id, frame.header());
        match frame.header().tag() {
            Tag::NewStream => {
                let stream_id = frame.header().stream_id();
                if self.streams.contains_key(&stream_id) {
                    log::error!(
                        "received NewStream message for existing stream: {}",
                        stream_id
                    );
                    return Err(ConnectionError::Io(std::io::ErrorKind::InvalidData.into()));
                }

                let (stream_sender, stream_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
                self.streams.insert(stream_id, stream_sender);
                let stream = Stream::new(
                    stream_id,
                    self.id,
                    self.stream_sender.clone(),
                    stream_receiver,
                );

                log::info!("{}: new inbound {} of {}", self.id, stream, self);
                if let Some(sender) = self.accept_stream.take() {
                    sender.send(Ok(stream));
                }
            }
            Tag::Message => {
                let stream_id = frame.header().stream_id();
                // if stream is closed, ignore frame
                if let Some(stream_sender) = self.streams.get_mut(&stream_id) {
                    stream_sender.send(frame.body().to_vec()).await;
                }
            }
            Tag::Reset | Tag::Close => {
                let stream_id = frame.header().stream_id();
                log::info!("{}: remote close stream {} of {}", self.id, stream_id, self);
                self.streams.remove(&stream_id);
            }
        };

        Ok(())
    }

    /// Process a command from one of our `Stream`s.
    async fn on_stream_command(&mut self, cmd: Option<StreamCommand>) -> Result<()> {
        match cmd {
            Some(StreamCommand::SendFrame(frame)) => {
                log::info!("{}: sending: {}", self.id, frame.header());
                self.socket
                    .send_frame(&frame)
                    .await
                    .or(Err(ConnectionError::Closed))?
            }
            Some(StreamCommand::CloseStream(id)) => {
                log::info!("{}: closing stream {} of {}", self.id, id, self);
                self.streams.remove(&id);
            }
            None => {
                // We only get to this point when `self.stream_receiver`
                // was closed which only happens in response to a close control
                // command. Now that we are at the end of the stream command queue,
                // we send the final term frame to the remote and complete the
                // closure.
                debug_assert!(self.control_receiver.is_paused());
                self.control_receiver.unpause();
                self.control_receiver.stream().close();
            }
        }
        Ok(())
    }

    /// Process a command from a `Control`.
    ///
    /// We only process control commands if we are not in the process of closing
    /// the connection. Only once we finished closing will we drain the remaining
    /// commands and reply back that we are closed.
    async fn on_control_command(&mut self, cmd: Option<ControlCommand>) -> Result<()> {
        match cmd {
            Some(ControlCommand::OpenStream(reply)) => {
                if self.shutdown.is_complete() {
                    // We are already closed so just inform the control.
                    let _ = reply.send(Err(ConnectionError::Closed));
                    return Ok(());
                }

                let stream_id = self.next_stream_id()?;
                let (stream_sender, stream_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
                self.streams.insert(stream_id, stream_sender);
                let stream = Stream::new(
                    stream_id,
                    self.id,
                    self.stream_sender.clone(),
                    stream_receiver,
                );

                log::info!("{}: new outbound {} of {}", self.id, stream, self);

                // send to peer with new stream frame
                let body = format!("{}", stream_id.val());
                let frame = Frame::new_stream_frame(stream_id, body.as_bytes());
                self.socket
                    .send_frame(&frame)
                    .await
                    .or(Err(ConnectionError::Closed))?;

                reply.send(Ok(stream));
            }
            Some(ControlCommand::AcceptStream(reply)) => {
                if self.accept_stream.is_none() {
                    self.accept_stream = Some(reply);
                }
            },
            Some(ControlCommand::CloseConnection(reply)) => {
                if self.shutdown.is_complete() {
                    // We are already closed so just inform the control.
                    let _ = reply.send(());
                    return Ok(());
                }
                debug_assert!(self.shutdown.has_not_started());
                self.shutdown = Shutdown::InProgress(reply);
                log::info!("closing connection {}", self);
                self.stream_receiver.close();
                self.control_receiver.pause();
            }
            None => {
                // We only get here after the whole connection shutdown is complete.
                // No further processing of commands of any kind or incoming frames
                // will happen.
                debug_assert!(self.shutdown.is_in_progress());
                log::info!("{}: closing {}", self.id, self);

                let shutdown = std::mem::replace(&mut self.shutdown, Shutdown::Complete);
                if let Shutdown::InProgress(tx) = shutdown {
                    // Inform the `Control` that initiated the shutdown.
                    let _ = tx.send(());
                }
                self.socket.close().await.or(Err(ConnectionError::Closed))?;

                return Err(ConnectionError::Closed);
            }
        }
        Ok(())
    }
}

impl<T> fmt::Display for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Connection {} (streams {}))",
            self.id,
            self.streams.len()
        )
    }
}

impl<T> Connection<T> {
    // next_stream_id is only used to get stream id when open stream
    fn next_stream_id(&mut self) -> Result<StreamID> {
        let proposed = StreamID::new(self.next_stream_id, true);
        self.next_stream_id = self
            .next_stream_id
            .checked_add(1)
            .ok_or(ConnectionError::NoMoreStreamIds)?;

        Ok(proposed)
    }

    /// Close and drop all `Stream`s and wake any pending `Waker`s.
    async fn drop_all_streams(&mut self) {
        log::info!("Drop all Streams count={}", self.streams.len());
        for (id, _sender) in self.streams.drain().take(1) {
            // drop it
            log::info!("drop stream {:?}", id);
        }
    }
}

// /// Turn a mplex [`Connection`] into a [`futures::Stream`].
// pub fn into_stream<T>(c: Connection<T>) -> impl futures::stream::Stream<Item = Result<Stream>>
// where
//     T: Read2 + Write2 + Unpin + Send,
// {
//     futures::stream::unfold(c, |mut c| async {
//         match c.next_stream().await {
//             Ok(None) => None,
//             Ok(Some(stream)) => Some((Ok(stream), c)),
//             Err(e) => Some((Err(e), c)),
//         }
//     })
// }
