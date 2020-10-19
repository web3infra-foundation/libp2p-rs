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

// This module contains the `Connection` type and associated helpers.
// A `Connection` wraps an underlying (async) I/O resource and multiplexes
// `Stream`s over it.
//
// The overall idea is as follows: The `Connection` makes progress via calls
// to its `next_stream` method which polls several futures, one that decodes
// `Frame`s from the I/O resource, one that consumes `ControlCommand`s
// from an MPSC channel and another one that consumes `StreamCommand`s from
// yet another MPSC channel. The latter channel is shared with every `Stream`
// created and whenever a `Stream` wishes to send a `Frame` to the remote end,
// it enqueues it into this channel (waiting if the channel is full). The
// former is shared with every `Control` clone and used to open new outbound
// streams or to trigger a connection close.
//
// The `Connection` updates the `Stream` state based on incoming frames, e.g.
// it pushes incoming data to the `Stream`'s buffer or increases the sending
// credit if the remote has sent us a corresponding `Frame::<WindowUpdate>`.
// Updating a `Stream`'s state acquires a `Mutex`, which every `Stream` has
// around its `Shared` state. While blocking, we make sure the lock is only
// held for brief moments and *never* while doing I/O. The only contention is
// between the `Connection` and a single `Stream`, which should resolve
// quickly. Ideally, we could use `futures::lock::Mutex` but it does not offer
// a poll-based API as of futures-preview 0.3.0-alpha.19, which makes it
// difficult to use in a `Stream`'s `AsyncRead` and `AsyncWrite` trait
// implementations.
//
// Closing a `Connection`
// ----------------------
//
// Every `Control` may send a `ControlCommand::Close` at any time and then
// waits on a `oneshot::Receiver` for confirmation that the connection is
// closed. The closing proceeds as follows:
//
// 1. As soon as we receive the close command we close the MPSC receiver
//    of `StreamCommand`s. We want to process any stream commands which are
//    already enqueued at this point but no more.
// 2. We change the internal shutdown state to `Shutdown::InProgress` which
//    contains the `oneshot::Sender` of the `Control` which triggered the
//    closure and which we need to notify eventually.
// 3. Crucially -- while closing -- we no longer process further control
//    commands, because opening new streams should no longer be allowed
//    and further close commands would mean we need to save those
//    `oneshot::Sender`s for later. On the other hand we also do not simply
//    close the control channel as this would signal to `Control`s that
//    try to send close commands, that the connection is already closed,
//    which it is not. So we just pause processing control commands which
//    means such `Control`s will wait.
// 4. We keep processing I/O and stream commands until the remaining stream
//    commands have all been consumed, at which point we transition the
//    shutdown state to `Shutdown::Complete`, which entails sending the
//    final termination frame to the remote, informing the `Control` and
//    now also closing the control channel.
// 5. Now that we are closed we go through all pending control commands
//    and tell the `Control`s that we are closed and we are finally done.
//
// While all of this may look complicated, it ensures that `Control`s are
// only informed about a closed connection when it really is closed.
//
// Potential improvements
// ----------------------
//
// There is always more work that can be done to make this a better crate,
// for example:
//
// - Instead of `futures::mpsc` a more efficient channel implementation
//   could be used, e.g. `tokio-sync`. Unfortunately `tokio-sync` is about
//   to be merged into `tokio` and depending on this large crate is not
//   attractive, especially given the dire situation around cargo's flag
//   resolution.
// - Flushing could be optimised. This would also require adding a
//   `StreamCommand::Flush` so that `Stream`s can trigger a flush, which
//   they would have to when they run out of credit, or else a series of
//   send operations might never finish.
// - If Rust gets async destructors, the `garbage_collect()` method can be
//   removed. Instead a `Stream` would send a `StreamCommand::Dropped(..)`
//   or something similar and the removal logic could happen within regular
//   command processing instead of having to scan the whole collection of
//   `Stream`s on each loop iteration, which is not great.

mod control;
mod stream;

use crate::{
    error::ConnectionError,
    frame::header::{self, Data, GoAway, Header, Ping, StreamId, Tag, WindowUpdate, CONNECTION_ID},
    frame::{self, Frame},
    pause::Pausable,
    Config, WindowUpdateMode, DEFAULT_CREDIT,
};
use futures::select;
use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    prelude::*,
    stream::FusedStream,
};

use nohash_hasher::IntMap;
use std::{fmt, sync::Arc};

pub use control::Control;
pub use stream::{State, Stream};

use libp2prs_traits::{ReadEx, WriteEx};
use std::collections::VecDeque;

/// Arbitrary limit of our internal command channels.
///
/// Since each `mpsc::Sender` gets a guaranteed slot in a channel the
/// actual upper bound is this value + number of clones.
const MAX_COMMAND_BACKLOG: usize = 32;

type Result<T> = std::result::Result<T, ConnectionError>;

/// How the connection is used.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Mode {
    /// Client to server connection.
    Client,
    /// Server to client connection.
    Server,
}

/// The connection identifier.
///
/// Randomly generated, this is mainly intended to improve log output.
#[derive(Clone, Copy)]
pub struct Id(u32, Mode);

impl Id {
    /// Create a random connection ID.
    pub(crate) fn random(mode: Mode) -> Self {
        Id(rand::random(), mode)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}({:08x})", self.1, self.0)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}({:08x})", self.1, self.0)
    }
}

/// A Yamux connection object.
///
/// Wraps the underlying I/O resource and makes progress via its
/// [`Connection::next_stream`] method which must be called repeatedly
/// until `Ok(None)` signals EOF or an error is encountered.
pub struct Connection<T> {
    id: Id,
    mode: Mode,
    config: Arc<Config>,
    socket: frame::Io<T>,
    next_id: u32,
    streams: IntMap<StreamId, Stream>,
    control_sender: mpsc::Sender<ControlCommand>,
    control_receiver: Pausable<mpsc::Receiver<ControlCommand>>,
    stream_sender: mpsc::UnboundedSender<StreamCommand>,
    stream_receiver: mpsc::UnboundedReceiver<StreamCommand>,
    waiting_stream_sender: Option<oneshot::Sender<Result<stream::Stream>>>,
    pending_streams: VecDeque<stream::Stream>,
    shutdown: Shutdown,
    is_closed: bool,
}

/// `Control` to `Connection` commands.
#[derive(Debug)]
pub(crate) enum ControlCommand {
    /// Open a new stream to the remote end.
    OpenStream(oneshot::Sender<Result<Stream>>),
    /// Accept a new stream from the remote end.
    AcceptStream(oneshot::Sender<Result<Stream>>),
    /// Close the whole connection.
    CloseConnection(oneshot::Sender<()>),
}

/// `Stream` to `Connection` commands.
#[derive(Debug)]
pub(crate) enum StreamCommand {
    /// A new frame should be sent to the remote.
    SendFrame(Frame<Either<Data, WindowUpdate>>),
    /// Close a stream.
    CloseStream { id: StreamId, ack: bool },
}

/// Possible actions as a result of incoming frame handling.
#[derive(Debug)]
enum Action {
    /// Nothing to be done.
    None,
    /// A new stream has been opened by the remote.
    New(Stream),
    /// A window update should be sent to the remote.
    Update(Frame<WindowUpdate>),
    /// A ping should be answered.
    Ping(Frame<Ping>),
    /// A stream should be reset.
    Reset(Frame<Data>),
    /// The connection should be terminated.
    Terminate(Frame<GoAway>),
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

impl<T> fmt::Debug for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .field("streams", &self.streams.len())
            .field("next_id", &self.next_id)
            .field("is_closed", &self.is_closed)
            .finish()
    }
}

impl<T> fmt::Display for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Connection {} {:?} (streams {}))", self.id, self.mode, self.streams.len())
    }
}

impl<T: ReadEx + WriteEx + Unpin + Send + 'static> Connection<T> {
    /// Create a new `Connection` from the given I/O resource.
    pub fn new(socket: T, cfg: Config, mode: Mode) -> Self {
        let id = Id::random(mode);
        log::debug!("new connection: {} ({:?})", id, mode);
        let (stream_sender, stream_receiver) = mpsc::unbounded();
        let (control_sender, control_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
        let socket = frame::Io::new(id, socket, cfg.max_buffer_size);
        Connection {
            id,
            mode,
            config: Arc::new(cfg),
            socket,
            streams: IntMap::default(),
            control_sender,
            control_receiver: Pausable::new(control_receiver),
            stream_sender,
            stream_receiver,
            next_id: match mode {
                Mode::Client => 1,
                Mode::Server => 2,
            },
            waiting_stream_sender: None,
            pending_streams: VecDeque::default(),
            shutdown: Shutdown::NotStarted,
            is_closed: false,
        }
    }

    /// Returns the id of the connection
    pub fn id(&self) -> Id {
        self.id
    }

    /// Get a controller for this connection.
    pub fn control(&self) -> Control {
        Control::new(self.control_sender.clone())
    }

    /// Get the next incoming stream, opened by the remote.
    ///
    /// This must be called repeatedly in order to make progress.
    /// Once `Ok(None)` or `Err(_)` is returned the connection is
    /// considered closed and no further invocation of this method
    /// must be attempted.
    ///
    /// # Cancellation
    ///
    /// Please note that if you poll the returned [`Future`] it *must
    /// not be cancelled* but polled until [`Poll::Ready`] is returned.
    pub async fn next_stream(&mut self) -> Result<()> {
        if self.is_closed {
            log::debug!("{}: connection is closed", self.id);
            return Ok(());
        }

        let result = self.next().await;
        log::error!("{}: error exit, {:?}", self.id, result);

        self.is_closed = true;

        if let Some(sender) = self.waiting_stream_sender.take() {
            sender.send(Err(ConnectionError::Closed)).expect("send err");
        }

        // At this point we are either at EOF or encountered an error.
        // We close all streams and wake up the associated tasks. We also
        // close and drain all receivers so no more commands can be
        // submitted. The connection is then considered closed.

        // Close and drain the control command receiver.
        if !self.control_receiver.stream().is_terminated() {
            self.control_receiver.stream().close();
            self.control_receiver.unpause();
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
            while let Some(_cmd) = self.stream_receiver.next().await {
                // drop it
            }
        }

        // if let Err(ConnectionError::Closed) = result {
        //     return Ok(());
        // }

        result
    }

    /// Get the next inbound `Stream` and make progress along the way.
    ///
    /// This is called from `Connection::next_stream` instead of being a
    /// public method itself in order to guarantee proper closing in
    /// case of an error or at EOF.
    async fn next(&mut self) -> Result<()> {
        loop {
            self.garbage_collect().await?;
            // Select all futures: socket, stream receiver and controller

            select! {
                frame = self.socket.recv_frame().fuse() => {
                    // if let Some(stream) = self.on_frame(Ok(frame.ok())).await? {
                    // recover from old code, why not return err when frame is Err
                    let frame = frame?;
                    self.on_frame(frame).await?;
                },
                scmd = self.stream_receiver.next() => {
                    self.on_stream_command(scmd).await?
                }
                ccmd = self.control_receiver.next() => {
                    self.on_control_command(ccmd).await?
                }
            };

            self.socket.flush().await.or(Err(ConnectionError::Closed))?
        }
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
                if self.streams.len() >= self.config.max_num_streams {
                    log::error!("{}: maximum number of streams reached", self.id);
                    let _ = reply.send(Err(ConnectionError::TooManyStreams));
                    return Ok(());
                }
                log::trace!("{}: creating new outbound stream", self.id);
                let id = self.next_stream_id()?;
                if !self.config.lazy_open {
                    let mut frame = Frame::window_update(id, self.config.receive_window);
                    frame.header_mut().syn();
                    log::trace!("{}: sending initial {}", self.id, frame.header());
                    self.socket.send_frame(&frame).await.or(Err(ConnectionError::Closed))?
                }
                let stream = {
                    let config = self.config.clone();
                    let sender = self.stream_sender.clone();
                    let window = self.config.receive_window;
                    let mut stream = Stream::new(id, self.id, config, window, DEFAULT_CREDIT, sender);
                    if self.config.lazy_open {
                        stream.set_flag(stream::Flag::Syn)
                    }
                    stream
                };
                if reply.send(Ok(stream.clone())).is_ok() {
                    log::debug!("{}: new outbound {} of {}", self.id, stream, self);
                    self.streams.insert(id, stream);
                } else {
                    log::debug!("{}: open stream {} has been cancelled", self.id, id);
                    if !self.config.lazy_open {
                        let mut header = Header::data(id, 0);
                        header.rst();
                        let frame = Frame::new(header);
                        self.socket.send_frame(&frame).await.or(Err(ConnectionError::Closed))?
                    }
                }
            }
            Some(ControlCommand::AcceptStream(reply)) => {
                if self.waiting_stream_sender.is_some() {
                    reply.send(Err(ConnectionError::Closed)).expect("send err");
                    return Ok(());
                }

                if let Some(stream) = self.pending_streams.pop_front() {
                    reply.send(Ok(stream)).expect("send err");
                } else {
                    self.waiting_stream_sender = Some(reply);
                }
            }
            Some(ControlCommand::CloseConnection(reply)) => {
                if self.shutdown.is_complete() {
                    // We are already closed so just inform the control.
                    let _ = reply.send(());
                    return Ok(());
                }
                // Handle initial close command.
                debug_assert!(self.shutdown.has_not_started());
                self.shutdown = Shutdown::InProgress(reply);
                log::trace!("{}: shutting down connection", self.id);
                self.control_receiver.pause();
                self.stream_receiver.close()
            }
            None => {
                // We only get here after the whole connection shutdown is complete.
                // No further processing of commands of any kind or incoming frames
                // will happen.
                debug_assert!(self.shutdown.is_complete());
                self.socket.close().await.or(Err(ConnectionError::Closed))?;
                return Err(ConnectionError::Closed);
            }
        }
        Ok(())
    }

    /// Process a command from one of our `Stream`s.
    async fn on_stream_command(&mut self, cmd: Option<StreamCommand>) -> Result<()> {
        match cmd {
            Some(StreamCommand::SendFrame(frame)) => {
                log::trace!("{}: sending: {}", self.id, frame.header());
                self.socket.send_frame(&frame).await.or(Err(ConnectionError::Closed))?
            }
            Some(StreamCommand::CloseStream { id, ack }) => {
                log::info!("{}: closing stream {} of {}", self.id, id, self);
                let mut header = Header::data(id, 0);
                header.fin();
                if ack {
                    header.ack()
                }
                let frame = Frame::new(header);
                self.socket.send_frame(&frame).await.or(Err(ConnectionError::Closed))?
            }
            None => {
                // We only get to this point when `self.stream_receiver`
                // was closed which only happens in response to a close control
                // command. Now that we are at the end of the stream command queue,
                // we send the final term frame to the remote and complete the
                // closure.
                debug_assert!(self.shutdown.is_in_progress());
                log::debug!("{}: closing {}", self.id, self);
                let frame = Frame::term();
                self.socket.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;
                let shutdown = std::mem::replace(&mut self.shutdown, Shutdown::Complete);
                if let Shutdown::InProgress(tx) = shutdown {
                    // Inform the `Control` that initiated the shutdown.
                    let _ = tx.send(());
                }
                debug_assert!(self.control_receiver.is_paused());
                self.control_receiver.unpause();
                self.control_receiver.stream().close()
            }
        }
        Ok(())
    }

    /// Process the result of reading from the socket.
    ///
    /// Unless `frame` is `Ok(Some(_))` we will assume the connection got closed
    /// and return a corresponding error, which terminates the connection.
    /// Otherwise we process the frame and potentially return a new `Stream`
    /// if one was opened by the remote.
    async fn on_frame(&mut self, frame: Frame<()>) -> Result<()> {
        log::trace!("{}: received: {}", self.id, frame.header());
        let action = match frame.header().tag() {
            Tag::Data => self.on_data(frame.into_data()).await?,
            Tag::WindowUpdate => self.on_window_update(&frame.into_window_update()).await?,
            Tag::Ping => self.on_ping(&frame.into_ping()),
            Tag::GoAway => return Err(ConnectionError::Closed),
        };
        match action {
            Action::None => {}
            Action::New(stream) => {
                log::trace!("{}: new inbound {} of {}", self.id, stream, self);
                if let Some(sender) = self.waiting_stream_sender.take() {
                    sender.send(Ok(stream)).expect("send err");
                } else {
                    self.pending_streams.push_back(stream);
                }
            }
            Action::Update(f) => {
                log::trace!("{}/{}: sending update", self.id, f.header().stream_id());
                self.socket.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
            Action::Ping(f) => {
                log::trace!("{}/{}: pong", self.id, f.header().stream_id());
                self.socket.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
            Action::Reset(f) => {
                log::trace!("{}/{}: sending reset", self.id, f.header().stream_id());
                self.socket.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
            Action::Terminate(f) => {
                log::trace!("{}: sending term", self.id);
                self.socket.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
        }
        Ok(())
    }

    async fn on_data(&mut self, frame: Frame<Data>) -> Result<Action> {
        let stream_id = frame.header().stream_id();

        log::trace!("{}/{}: received {:?}", self.id, stream_id, frame);

        if frame.header().flags().contains(header::RST) {
            // stream reset
            if let Some(s) = self.streams.get_mut(&stream_id) {
                let shared = s.shared();
                shared.update_state(self.id, stream_id, State::Closed);
                shared.notify_send()?;
                shared.notify_recv()?;
            }
            return Ok(Action::None);
        }

        let is_finish = frame.header().flags().contains(header::FIN); // half-close

        if frame.header().flags().contains(header::SYN) {
            // new stream
            if !self.is_valid_remote_id(stream_id, Tag::Data) {
                log::error!("{}: invalid stream id {}", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if frame.body().len() > DEFAULT_CREDIT as usize {
                log::error!("{}/{}: 1st body of stream exceeds default credit", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if self.streams.contains_key(&stream_id) {
                log::error!("{}/{}: stream already exists", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if self.streams.len() == self.config.max_num_streams {
                log::error!("{}: maximum number of streams reached", self.id);
                return Ok(Action::Terminate(Frame::internal_error()));
            }
            let stream = {
                let config = self.config.clone();
                let credit = DEFAULT_CREDIT;
                let sender = self.stream_sender.clone();
                let mut stream = Stream::new(stream_id, self.id, config, credit, credit, sender);
                stream.set_flag(stream::Flag::Ack);
                stream
            };
            {
                let shared = stream.shared();
                if is_finish {
                    shared.update_state(self.id, stream_id, State::RecvClosed);
                }
                shared.sub_window(frame.body_len());
                shared.buffer().await.push(frame.into_body());
            }
            self.streams.insert(stream_id, stream.clone());
            return Ok(Action::New(stream));
        }

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            let shared = stream.shared();
            if frame.body().len() > shared.window() as usize {
                log::error!("{}/{}: frame body larger than window of stream", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if is_finish {
                shared.update_state(self.id, stream_id, State::RecvClosed);
            }
            let max_buffer_size = self.config.max_buffer_size;
            if shared.buffer().await.len().map(move |n| n >= max_buffer_size).unwrap_or(true) {
                log::error!("{}/{}: buffer of stream grows beyond limit", self.id, stream_id);
                let mut header = Header::data(stream_id, 0);
                header.rst();
                return Ok(Action::Reset(Frame::new(header)));
            }
            shared.sub_window(frame.body_len());
            shared.buffer().await.push(frame.into_body());

            shared.notify_recv()?;

            if !is_finish && shared.window() == 0 && self.config.window_update_mode == WindowUpdateMode::OnReceive {
                let frame = Frame::window_update(stream_id, self.config.receive_window);
                shared.update_window(self.config.receive_window);
                return Ok(Action::Update(frame));
            }
        } else if !is_finish {
            log::debug!("{}/{}: data for unknown stream", self.id, stream_id);
            let mut header = Header::data(stream_id, 0);
            header.rst();
            return Ok(Action::Reset(Frame::new(header)));
        }

        Ok(Action::None)
    }

    async fn on_window_update(&mut self, frame: &Frame<WindowUpdate>) -> Result<Action> {
        let stream_id = frame.header().stream_id();

        if frame.header().flags().contains(header::RST) {
            // stream reset
            if let Some(s) = self.streams.get_mut(&stream_id) {
                let shared = s.shared();
                shared.update_state(self.id, stream_id, State::Closed);

                shared.notify_send()?;
                shared.notify_recv()?;
            }
            return Ok(Action::None);
        }

        let is_finish = frame.header().flags().contains(header::FIN); // half-close

        if frame.header().flags().contains(header::SYN) {
            // new stream
            if !self.is_valid_remote_id(stream_id, Tag::WindowUpdate) {
                log::error!("{}: invalid stream id {}", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if self.streams.contains_key(&stream_id) {
                log::error!("{}/{}: stream already exists", self.id, stream_id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            if self.streams.len() == self.config.max_num_streams {
                log::error!("{}: maximum number of streams reached", self.id);
                return Ok(Action::Terminate(Frame::protocol_error()));
            }
            let stream = {
                let credit = frame.header().credit();
                let config = self.config.clone();
                let sender = self.stream_sender.clone();
                let mut stream = Stream::new(stream_id, self.id, config, DEFAULT_CREDIT, credit, sender);
                stream.set_flag(stream::Flag::Ack);
                stream
            };
            if is_finish {
                stream.shared().update_state(self.id, stream_id, State::RecvClosed);
            }
            self.streams.insert(stream_id, stream.clone());
            return Ok(Action::New(stream));
        }

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            let shared = stream.shared();
            shared.add_credit(frame.header().credit());
            if is_finish {
                shared.update_state(self.id, stream_id, State::RecvClosed);
            }
            shared.notify_send()?;
        } else if !is_finish {
            log::debug!("{}/{}: window update for unknown stream", self.id, stream_id);
            let mut header = Header::data(stream_id, 0);
            header.rst();
            return Ok(Action::Reset(Frame::new(header)));
        }

        Ok(Action::None)
    }

    fn on_ping(&mut self, frame: &Frame<Ping>) -> Action {
        let stream_id = frame.header().stream_id();
        if frame.header().flags().contains(header::ACK) {
            // pong
            return Action::None;
        }
        if stream_id == CONNECTION_ID || self.streams.contains_key(&stream_id) {
            let mut hdr = Header::ping(frame.header().nonce());
            hdr.ack();
            return Action::Ping(Frame::new(hdr));
        }
        log::debug!("{}/{}: ping for unknown stream", self.id, stream_id);
        let mut header = Header::data(stream_id, 0);
        header.rst();
        Action::Reset(Frame::new(header))
    }

    fn next_stream_id(&mut self) -> Result<StreamId> {
        let proposed = StreamId::new(self.next_id);
        self.next_id = self.next_id.checked_add(2).ok_or(ConnectionError::NoMoreStreamIds)?;
        match self.mode {
            Mode::Client => assert!(proposed.is_client()),
            Mode::Server => assert!(proposed.is_server()),
        }
        Ok(proposed)
    }

    // Check if the given stream ID is valid w.r.t. the provided tag and our connection mode.
    fn is_valid_remote_id(&self, id: StreamId, tag: Tag) -> bool {
        if tag == Tag::Ping || tag == Tag::GoAway {
            return id.is_session();
        }
        match self.mode {
            Mode::Client => id.is_server(),
            Mode::Server => id.is_client(),
        }
    }

    /// Remove stale streams and send necessary messages to the remote.
    ///
    /// If we ever get async destructors we can replace this with streams
    /// sending a proper command when dropped.
    async fn garbage_collect(&mut self) -> Result<()> {
        let conn_id = self.id;
        let win_update_mode = self.config.window_update_mode;
        let mut garbage = Vec::new();

        for stream in self.streams.values_mut() {
            if stream.strong_count() > 1 {
                continue;
            }
            log::info!("{}: removing dropped {}", conn_id, stream);
            let stream_id = stream.id();
            let frame = {
                let shared = stream.shared();
                let frame = match shared.update_state(conn_id, stream_id, State::Closed) {
                    // The stream was dropped without calling `poll_close`.
                    // We reset the stream to inform the remote of the closure.
                    State::Open => {
                        let mut header = Header::data(stream_id, 0);
                        header.rst();
                        Some(Frame::new(header))
                    }
                    // The stream was dropped without calling `poll_close`.
                    // We have already received a FIN from remote and send one
                    // back which closes the stream for good.
                    State::RecvClosed => {
                        let mut header = Header::data(stream_id, 0);
                        header.fin();
                        Some(Frame::new(header))
                    }
                    // The stream was properly closed. We either already have
                    // or will at some later point send our FIN frame.
                    // The remote may be out of credit though and blocked on
                    // writing more data. We may need to reset the stream.
                    State::SendClosed => {
                        if win_update_mode == WindowUpdateMode::OnRead && shared.window() == 0 {
                            // The stream has unconsumed data left when closed.
                            // The remote may be waiting for a window update
                            // which we will never send, so reset the stream now.
                            let mut header = Header::data(stream_id, 0);
                            header.rst();
                            Some(Frame::new(header))
                        } else {
                            // The remote is not blocked as we send window updates
                            // for as long as we know the stream. For unknown streams
                            // we send a RST in `Connection::on_data`.
                            // For `OnRead` and empty stream buffers we have or will
                            // send another window update too.
                            None
                        }
                    }
                    // The stream was properly closed. We either already have
                    // or will at some later point send our FIN frame. The
                    // remote end has already done so in the past.
                    State::Closed => None,
                };
                shared.notify_recv()?;
                shared.notify_send()?;
                frame
            };
            if let Some(f) = frame {
                // self.socket.send_frame(&f).await.or(Err(ConnectionError::Closed))?
                let cmd = StreamCommand::SendFrame(f.left());
                self.stream_sender.send(cmd).await.or(Err(ConnectionError::Closed))?;
            }
            garbage.push(stream_id)
        }
        for id in garbage.drain(..) {
            self.streams.remove(&id);
        }
        Ok(())
    }
}

impl<T> Connection<T> {
    /// Close and drop all `Stream`s and wake any pending `Waker`s.
    async fn drop_all_streams(&mut self) {
        log::info!("Drop all Streams and wake any pending Wakers, count={}", self.streams.len());
        for (id, s) in self.streams.drain() {
            let shared = s.shared();
            shared.update_state(self.id, id, State::Closed);

            if let Err(e) = shared.notify_recv() {
                log::error!("failure notify stream recv {}", e);
            }
            if let Err(e) = shared.notify_send() {
                log::error!("failure notify stream send {}", e);
            }
        }
    }
}

impl<T> Drop for Connection<T> {
    fn drop(&mut self) {
        log::trace!("{:?} dropped", self.id);
        // probably we don't have to call drop_all_... , since all streams will be
        // cleaned up when the underlying socket is down
        //self.drop_all_streams();
    }
}

// /// Turn a Yamux [`Connection`] into a [`futures::Stream`].
// pub fn into_stream<T>(c: Connection<T>) -> impl futures::stream::Stream<Item = Result<Stream>>
// where
//     T: ReadEx + WriteEx + Unpin + Send,
// {
//     futures::stream::unfold(c, |mut c| async {
//         match c.next_stream().await {
//             Ok(None) => None,
//             Ok(Some(stream)) => Some((Ok(stream), c)),
//             Err(e) => Some((Err(e), c)),
//         }
//     })
// }
