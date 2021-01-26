// Copyright 2018 Parity Technologies (UK) Ltd.
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
// it pushes incoming data to the `Stream` via channel or increases the sending
// credit if the remote has sent us a corresponding `Frame::<WindowUpdate>`.
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
// specific
// ----------------------
// - All stream's state is managed by connection, stream state get from channel
//   Shared lock is not efficient.
// - Connection pushes incoming data to the `Stream` via channel, not buffer
// - Stream must be closed explicitly Since garbage collect is not implemented.
//   Drop it directly do nothing
//
// Potential improvements
// ----------------------
//
// There is always more work that can be done to make this a better crate,
// for example:
// - Loop in handle_coming() is performance bottleneck.  More seriously, it will be block
//   when two peers echo with mass of data with lot of stream Since they block
//   on write data and none of them can read data.
//   One solution is spawn runtime for reader and writer But depend on async runtime
//   is not attractive. See detail from concurrent in tests

pub mod control;
pub mod stream;

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    select,
    stream::FusedStream,
};

use crate::connection::stream::Flag;
use crate::{
    error::ConnectionError,
    frame::header::{self, Data, GoAway, Header, Ping, StreamId, Tag, WindowUpdate, CONNECTION_ID},
    frame::{io, Frame, FrameDecodeError},
    pause::Pausable,
    Config, WindowUpdateMode, DEFAULT_CREDIT,
};
use control::Control;
use libp2prs_traits::{SplitEx, SplittableReadWrite};
use nohash_hasher::IntMap;
use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use stream::{State, Stream, StreamStat};

/// `Control` to `Connection` commands.
#[derive(Debug)]
pub enum ControlCommand {
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
    SendFrame(Frame<Data>, oneshot::Sender<usize>),
    /// Close a stream.
    CloseStream(StreamId, oneshot::Sender<()>),
    /// Reset a stream.
    ResetStream(StreamId, oneshot::Sender<()>),
}

/// Possible actions as a result of incoming frame handling.
#[derive(Debug)]
enum Action {
    /// Nothing to be done.
    None,
    /// A new stream has been opened by the remote.
    New(Stream, Option<Frame<WindowUpdate>>),
    /// A window update should be sent to the remote.
    Update(Frame<WindowUpdate>),
    /// A ping should be answered.
    Ping(Frame<Ping>),
    /// A stream should be reset.
    Reset(StreamId),
    /// The connection should be terminated.
    Terminate(Frame<GoAway>),
}

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
        matches!(self, Shutdown::NotStarted)
    }

    fn is_in_progress(&self) -> bool {
        matches!(self, Shutdown::InProgress(_))
    }

    fn is_complete(&self) -> bool {
        matches!(self, Shutdown::Complete)
    }
}

type Result<T> = std::result::Result<T, ConnectionError>;

pub struct Connection<T: SplitEx> {
    id: Id,
    mode: Mode,
    config: Arc<Config>,
    reader: Pin<Box<dyn FusedStream<Item = std::result::Result<Frame<()>, FrameDecodeError>> + Send>>,
    // writer: io::Io<WriteHalf<T>>,
    writer: io::Io<T::Writer>,
    is_closed: bool,
    shutdown: Shutdown,
    next_stream_id: u32,
    streams: IntMap<StreamId, mpsc::UnboundedSender<Vec<u8>>>,
    streams_stat: IntMap<StreamId, StreamStat>,
    stream_sender: mpsc::UnboundedSender<StreamCommand>,
    stream_receiver: mpsc::UnboundedReceiver<StreamCommand>,
    control_sender: mpsc::UnboundedSender<ControlCommand>,
    control_receiver: Pausable<mpsc::UnboundedReceiver<ControlCommand>>,
    waiting_stream_sender: Option<oneshot::Sender<Result<Stream>>>,
    pending_streams: VecDeque<Stream>,
    pending_frames: IntMap<StreamId, (Frame<Data>, oneshot::Sender<usize>)>,
}

impl<T: SplittableReadWrite> Connection<T> {
    /// Create a new `Connection` from the given I/O resource.
    pub fn new(socket: T, cfg: Config, mode: Mode) -> Self {
        let id = Id::random(mode);
        log::debug!("new connection: {}", id);

        let (reader, writer) = socket.split();
        // let (reader, writer) = socket.split2();
        let reader = io::Io::new(id, reader, cfg.max_buffer_size);
        let reader = futures::stream::unfold(reader, |mut io| async { Some((io.recv_frame().await, io)) });
        let reader = Box::pin(reader);

        let writer = io::Io::new(id, writer, cfg.max_buffer_size);
        let (stream_sender, stream_receiver) = mpsc::unbounded();
        let (control_sender, control_receiver) = mpsc::unbounded();

        Connection {
            id,
            mode,
            config: Arc::new(cfg),
            reader,
            writer,
            is_closed: false,
            next_stream_id: match mode {
                Mode::Client => 1,
                Mode::Server => 2,
            },
            shutdown: Shutdown::NotStarted,
            streams: IntMap::default(),
            streams_stat: IntMap::default(),
            stream_sender,
            stream_receiver,
            control_sender,
            control_receiver: Pausable::new(control_receiver),
            waiting_stream_sender: None,
            pending_streams: VecDeque::default(),
            pending_frames: IntMap::default(),
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
    /// Once `Ok(()))` or `Err(_)` is returned the connection is
    /// considered closed and no further invocation of this method
    /// must be attempted.
    pub async fn next_stream(&mut self) -> Result<()> {
        if self.is_closed {
            log::debug!("{}: connection is closed", self.id);
            return Ok(());
        }

        let result = self.handle_coming().await;
        log::debug!("{}: error exit, {:?}", self.id, result);

        self.is_closed = true;

        if let Some(sender) = self.waiting_stream_sender.take() {
            sender.send(Err(ConnectionError::Closed)).expect("send err");
        }

        self.drop_pending_frame();

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
            while let Some(cmd) = self.stream_receiver.next().await {
                match cmd {
                    StreamCommand::SendFrame(_, reply) => {
                        let _ = reply.send(0);
                    }
                    StreamCommand::CloseStream(_, reply) => {
                        let _ = reply.send(());
                    }
                    StreamCommand::ResetStream(_, reply) => {
                        let _ = reply.send(());
                    }
                }
                // drop it
                log::trace!("drop stream receiver frame");
            }
        }

        result
    }

    /// This is called from `Connection::next_stream` instead of being a
    /// public method itself in order to guarantee proper closing in
    /// case of an error or at EOF.
    async fn handle_coming(&mut self) -> Result<()> {
        loop {
            select! {
                // handle incoming
                frame = self.reader.next() => {
                    if let Some(f) = frame {
                        let frame = f?;
                        self.on_frame(frame).await?;
                    }
                }
                // handle outcoming
                scmd = self.stream_receiver.next() => {
                    self.on_stream_command(scmd).await?;
                }
                ccmd = self.control_receiver.next() => {
                    self.on_control_command(ccmd).await?;
                }
            }
            self.writer.flush().await.or(Err(ConnectionError::Closed))?
        }
    }

    /// Process the result of reading from the socket.
    ///
    /// Unless `frame` is `Ok(()))` we will assume the connection got closed
    /// and return a corresponding error, which terminates the connection.
    /// Otherwise we process the frame
    async fn on_frame(&mut self, frame: Frame<()>) -> Result<()> {
        log::debug!("{}: received: {}", self.id, frame.header());
        let action = match frame.header().tag() {
            Tag::Data => self.on_data(frame.into_data()).await,
            Tag::WindowUpdate => self.on_window_update(&frame.into_window_update()).await,
            Tag::Ping => self.on_ping(&frame.into_ping()),
            Tag::GoAway => return Err(ConnectionError::Closed),
        };
        match action {
            Action::None => {}
            Action::New(stream, update) => {
                log::trace!("{}: new inbound {} of {}", self.id, stream, self);
                if let Some(sender) = self.waiting_stream_sender.take() {
                    sender.send(Ok(stream)).expect("send err");
                } else {
                    self.pending_streams.push_back(stream);
                }

                if let Some(f) = update {
                    log::trace!("{}/{}: sending update", self.id, f.header().stream_id());
                    self.writer.send_frame(&f).await.or(Err(ConnectionError::Closed))?
                }
            }
            Action::Update(f) => {
                log::debug!("{}/{}: sending update", self.id, f.header().stream_id());
                self.writer.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
            Action::Ping(f) => {
                log::trace!("{}/{}: pong", self.id, f.header().stream_id());
                self.writer.send_frame(&f).await.or(Err(ConnectionError::Closed))?
            }
            Action::Reset(stream_id) => {
                log::trace!("{}/{}: sending reset", self.id, stream_id);
                self.send_reset_stream(stream_id).await.or(Err(ConnectionError::Closed))?
            }
            Action::Terminate(f) => {
                log::trace!("{}: sending term", self.id);

                // yamux crate just send GoAway, while go-yamux session will exitErr at this time.
                // I think connecttion is useless now, so exit
                self.writer.send_frame(&f).await.or(Err(ConnectionError::Closed))?;
                return Err(ConnectionError::Closed);
            }
        }
        Ok(())
    }

    /// Process new inbound stream when recv Data frame
    async fn new_stream_data(&mut self, stream_id: StreamId, is_finish: bool, frame_body: Vec<u8>) -> Action {
        let frame_len = frame_body.len();
        if !self.is_valid_remote_id(stream_id, Tag::Data) {
            // log::error!("{}: invalid stream id {}", self.id, stream_id);
            return Action::Terminate(Frame::protocol_error());
        }
        if frame_len > DEFAULT_CREDIT as usize {
            log::error!("{}/{}: 1st body of stream exceeds default credit", self.id, stream_id);
            return Action::Terminate(Frame::protocol_error());
        }
        if self.streams.contains_key(&stream_id) {
            log::error!("{}/{}: stream already exists", self.id, stream_id);
            return Action::Terminate(Frame::protocol_error());
        }
        if self.streams.len() == self.config.max_num_streams {
            log::error!("{}: maximum number of streams reached", self.id);
            return Action::Terminate(Frame::internal_error());
        }
        if self.streams_stat.contains_key(&stream_id) {
            log::error!("received NewStream message for existing stream: {}", stream_id);
            return Action::Terminate(Frame::protocol_error());
        }

        let (mut stream_sender, stream_receiver) = mpsc::unbounded();

        let stream = Stream::new(stream_id, self.id, self.config.clone(), self.stream_sender.clone(), stream_receiver);

        let mut stream_stat = StreamStat::new(DEFAULT_CREDIT, DEFAULT_CREDIT as usize);

        // support lazy_open, peer can send data with SYN flag
        let window_update;
        {
            if is_finish {
                stream_stat.update_state(self.id, stream_id, State::RecvClosed);
                self.streams.remove(&stream_id);
            }
            stream_stat.window = stream_stat.window.saturating_sub(frame_len as u32);
            let _ = stream_sender.send(frame_body).await;
            if !is_finish && stream_stat.window == 0 && self.config.window_update_mode == WindowUpdateMode::OnReceive {
                stream_stat.window = self.config.receive_window;
                let mut frame = Frame::window_update(stream_id, self.config.receive_window);
                frame.header_mut().ack();
                window_update = Some(frame)
            } else {
                window_update = None
            }
        }
        if window_update.is_none() {
            stream_stat.set_flag(stream::Flag::Ack)
        }

        self.streams.insert(stream_id, stream_sender);
        self.streams_stat.insert(stream_id, stream_stat);
        Action::New(stream, window_update)
    }

    /// Process received Data frame
    async fn on_data(&mut self, frame: Frame<Data>) -> Action {
        let stream_id = frame.header().stream_id();
        let flags = frame.header().flags();
        let frame_len = frame.body_len();

        log::trace!("{}/{}: received {:?}", self.id, stream_id, frame.header());

        // stream reset
        if flags.contains(header::RST) {
            self.streams.remove(&stream_id);
            self.streams_stat.remove(&stream_id);
            return Action::None;
        }

        let is_finish = frame.header().flags().contains(header::FIN); // half-close

        // new stream
        if flags.contains(header::SYN) {
            return self.new_stream_data(stream_id, is_finish, frame.into_body()).await;
        }

        // flag to remove stat from streams_stat
        let mut rm = false;
        if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
            if frame_len > stat.window {
                log::error!("{}/{}: frame body larger than window of stream", self.id, stream_id);
                return Action::Terminate(Frame::protocol_error());
            }

            stat.window = stat.window.saturating_sub(frame_len);

            if frame_len > 0 && (self.config.read_after_close || stat.state().can_read()) {
                if let Some(sender) = self.streams.get_mut(&stream_id) {
                    let _ = sender.send(frame.into_body()).await;
                }
            }

            if is_finish {
                self.streams.remove(&stream_id);
                let prev = stat.update_state(self.id, stream_id, State::RecvClosed);
                if prev == State::SendClosed {
                    rm = true;
                }
            }

            if !is_finish && stat.window == 0 && self.config.window_update_mode == WindowUpdateMode::OnReceive {
                stat.window = self.config.receive_window;
                let frame = Frame::window_update(stream_id, self.config.receive_window);
                return Action::Update(frame);
            }
        } else if !is_finish {
            log::debug!("{}/{}: data for unknown stream", self.id, stream_id);
            return Action::Reset(stream_id);
        }

        // If stream is completely closed, remove it
        if rm {
            self.streams_stat.remove(&stream_id);
            self.pending_frames.remove(&stream_id);
        }

        Action::None
    }

    /// Process new inbound stream when recv window update frame
    fn new_stream_window_update(&mut self, stream_id: StreamId, is_finish: bool) -> Action {
        if !self.is_valid_remote_id(stream_id, Tag::WindowUpdate) {
            // log::error!("{}: invalid stream id {}", self.id, stream_id);
            return Action::Terminate(Frame::protocol_error());
        }
        if self.streams.contains_key(&stream_id) {
            log::error!("{}/{}: stream already exists", self.id, stream_id);
            return Action::Terminate(Frame::protocol_error());
        }
        if self.streams.len() == self.config.max_num_streams {
            // log::error!("{}: maximum number of streams reached", self.id);
            return Action::Terminate(Frame::internal_error());
        }
        if self.streams_stat.contains_key(&stream_id) {
            log::error!("received NewStream message for existing stream: {}", stream_id);
            return Action::Terminate(Frame::protocol_error());
        }

        let (stream_sender, stream_receiver) = mpsc::unbounded();
        self.streams.insert(stream_id, stream_sender);

        let stream = Stream::new(stream_id, self.id, self.config.clone(), self.stream_sender.clone(), stream_receiver);

        // in order to communicate with go-yamux, ignore len of windows update frame, credit is always DEFAULT_CREDIT
        let mut stream_stat = StreamStat::new(DEFAULT_CREDIT, DEFAULT_CREDIT as usize);
        stream_stat.set_flag(Flag::Ack);

        if is_finish {
            stream_stat.update_state(self.id, stream_id, State::RecvClosed);
            self.streams.remove(&stream_id);
        }

        self.streams_stat.insert(stream_id, stream_stat);
        Action::New(stream, None)
    }

    /// Process received window update frame
    async fn on_window_update(&mut self, frame: &Frame<WindowUpdate>) -> Action {
        let mut updated = false;
        let stream_id = frame.header().stream_id();
        let flags = frame.header().flags();

        if flags.contains(header::RST) {
            // stream reset
            self.streams.remove(&stream_id);
            self.streams_stat.remove(&stream_id);
            return Action::None;
        }

        let is_finish = frame.header().flags().contains(header::FIN); // half-close

        // new stream
        if flags.contains(header::SYN) {
            return self.new_stream_window_update(stream_id, is_finish);
        }

        // flag to remove stat from streams_stat
        let mut rm = false;
        if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
            stat.credit += frame.header().credit();
            updated = true;
            if is_finish {
                self.streams.remove(&stream_id);
                let prev = stat.update_state(self.id, stream_id, State::RecvClosed);
                if prev == State::SendClosed {
                    rm = true;
                }
            }
        } else if !is_finish {
            log::debug!("{}/{}: window update for unknown stream", self.id, stream_id);
            return Action::Reset(stream_id);
        }

        // If stream is completely closed, remove it
        if rm {
            self.streams_stat.remove(&stream_id);
            self.pending_frames.remove(&stream_id);
        } else if updated {
            // If window size is updated, send pending frames
            if let Some((frame, reply)) = self.pending_frames.remove(&stream_id) {
                let _ = self.send_frame(frame, reply).await;
            }
        }

        Action::None
    }

    /// Process received Ping frame
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
        Action::Reset(stream_id)
    }

    /// reset stream
    async fn send_reset_stream(&mut self, stream_id: StreamId) -> Result<()> {
        // step1: send close frame
        let mut header = Header::data(stream_id, 0);
        header.rst();
        let frame = Frame::new(header);
        self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

        // step2: remove stream
        self.streams_stat.remove(&stream_id);
        self.streams.remove(&stream_id);

        Ok(())
    }

    /// send frame
    async fn send_frame(&mut self, mut frame: Frame<Data>, reply: oneshot::Sender<usize>) -> Result<()> {
        let stream_id = frame.header().stream_id();
        if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
            if stat.state().can_write() {
                if stat.credit == 0 {
                    if self.pending_frames.contains_key(&stream_id) {
                        let _ = reply.send(0);
                        return Ok(());
                    }
                    self.pending_frames.insert(stream_id, (frame, reply));
                    return Ok(());
                }

                let len = frame.body().len();
                let n = std::cmp::min(stat.credit, len);
                if n < len {
                    frame.truncate(n);
                }

                stat.credit -= n;
                send_flag(stat, &mut frame);
                self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;
                log::debug!("{}: sending: {}", self.id, frame.header());
                let _ = reply.send(n);
            } else {
                log::debug!("{}: stream {} have been removed", self.id, stream_id);
                drop(reply);
            }
        } else {
            drop(reply);
        }
        Ok(())
    }

    /// Process a command from one of our `Stream`s.
    async fn on_stream_command(&mut self, cmd: Option<StreamCommand>) -> Result<()> {
        match cmd {
            Some(StreamCommand::SendFrame(frame, reply)) => {
                self.send_frame(frame, reply).await?;
            }
            Some(StreamCommand::CloseStream(stream_id, reply)) => {
                log::debug!("{}: closing stream {} of {}", self.id, stream_id, self);
                // flag to remove stat from streams_stat
                let mut rm = false;
                if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
                    if stat.state().can_write() {
                        // step1: send close frame
                        let mut header = Header::data(stream_id, 0);
                        header.fin();
                        if stat.get_flag() == Flag::Ack {
                            header.ack();
                            stat.set_flag(Flag::None);
                        }
                        let frame = Frame::new(header);
                        self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

                        // step2: update state
                        let prev = stat.update_state(self.id, stream_id, State::SendClosed);
                        if prev == State::RecvClosed {
                            rm = true;
                        }
                    }
                }
                // If stream is completely closed, remove it
                if rm {
                    self.streams_stat.remove(&stream_id);
                    self.pending_frames.remove(&stream_id);
                }
                let _ = reply.send(());
            }
            Some(StreamCommand::ResetStream(stream_id, reply)) => {
                log::debug!("{}: reset stream {} of {}", self.id, stream_id, self);
                if self.streams_stat.contains_key(&stream_id) {
                    self.send_reset_stream(stream_id).await?;
                }
                let _ = reply.send(());
            }
            None => {
                // We only get to this point when `self.stream_receiver`
                // was closed which only happens in response to a close control
                // command. Now that we are at the end of the stream command queue,
                // we send the final term frame to the remote and complete the
                // closure.
                debug_assert!(self.control_receiver.is_paused());
                let frame = Frame::term();
                self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

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
                if self.streams.len() >= self.config.max_num_streams {
                    // log::error!("{}: maximum number of streams reached", self.id);
                    let _ = reply.send(Err(ConnectionError::TooManyStreams));
                    return Ok(());
                }
                log::trace!("{}: creating new outbound stream", self.id);
                let stream_id = self.next_stream_id()?;
                let window = self.config.receive_window;
                let mut stream_stat = StreamStat::new(window, DEFAULT_CREDIT as usize);
                if !self.config.lazy_open {
                    let mut frame = Frame::window_update(stream_id, self.config.receive_window);
                    frame.header_mut().syn();
                    log::trace!("{}: sending initial {}", self.id, frame.header());
                    self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?
                } else {
                    stream_stat.set_flag(Flag::Syn);
                }

                let (stream_sender, stream_receiver) = mpsc::unbounded();

                let stream = Stream::new(stream_id, self.id, self.config.clone(), self.stream_sender.clone(), stream_receiver);
                if reply.send(Ok(stream)).is_ok() {
                    log::debug!("{}: new outbound {}", self.id, stream_id);
                    self.streams.insert(stream_id, stream_sender);
                    self.streams_stat.insert(stream_id, stream_stat);
                } else {
                    log::debug!("{}: open stream {} has been cancelled", self.id, stream_id);
                    if !self.config.lazy_open {
                        self.send_reset_stream(stream_id).await?;
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
                if !self.shutdown.has_not_started() {
                    log::debug!("shutdown had started, ingore this request");
                    let _ = reply.send(());
                    return Ok(());
                }
                self.shutdown = Shutdown::InProgress(reply);
                log::debug!("closing connection {}", self);
                self.stream_receiver.close();
                self.control_receiver.pause();
            }
            None => {
                // We only get here after the whole connection shutdown is complete.
                // No further processing of commands of any kind or incoming frames
                // will happen.
                debug_assert!(self.shutdown.is_in_progress());
                log::debug!("{}: closing {}", self.id, self);

                let shutdown = std::mem::replace(&mut self.shutdown, Shutdown::Complete);
                if let Shutdown::InProgress(tx) = shutdown {
                    // Inform the `Control` that initiated the shutdown.
                    let _ = tx.send(());
                }
                self.writer.close().await.or(Err(ConnectionError::Closed))?;

                return Err(ConnectionError::Closed);
            }
        }
        Ok(())
    }
}

fn send_flag(stat: &mut StreamStat, frame: &mut Frame<Data>) {
    // send ack/syn along with frame
    match stat.get_flag() {
        Flag::None => (),
        Flag::Syn => {
            frame.header_mut().syn();
            stat.set_flag(Flag::None);
        }
        Flag::Ack => {
            frame.header_mut().ack();
            stat.set_flag(Flag::None);
        }
    }
}

impl<T: SplitEx> fmt::Display for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Connection {} (streams {}))", self.id, self.streams.len())
    }
}

impl<T: SplitEx> Connection<T> {
    // next_stream_id is only used to get stream id when open stream
    fn next_stream_id(&mut self) -> Result<StreamId> {
        let proposed = StreamId::new(self.next_stream_id);
        self.next_stream_id = self.next_stream_id.checked_add(2).ok_or(ConnectionError::NoMoreStreamIds)?;
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

    pub fn streams_length(&self) -> usize {
        self.streams_stat.len()
    }

    /// Close and drop all `Stream`s sender and stat.
    async fn drop_all_streams(&mut self) {
        log::trace!("{}: Drop all Streams sender count={}", self.id, self.streams.len());
        for (id, _sender) in self.streams.drain().take(1) {
            // drop it
            log::trace!("{}: drop stream sender {:?}", self.id, id);
        }

        log::trace!("{}: Drop all Streams stat count={}", self.id, self.streams.len());
        for (id, _stat) in self.streams_stat.drain().take(1) {
            // drop it
            log::trace!("{}: drop stream stat {:?}", self.id, id);
        }
    }

    fn drop_pending_frame(&mut self) {
        log::trace!("{}: Drop all pengding frame count={}", self.id, self.pending_frames.len());
        for (stream_id, (_, reply)) in self.pending_frames.drain().take(1) {
            log::trace!("{} drop pengding frame of stream {:?}", self.id, stream_id);
            let _ = reply.send(0);
        }
    }
}
