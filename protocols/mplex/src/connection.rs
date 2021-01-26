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
// - All stream's state is managed by connecttion, stream state get from channel
//   Shared lock is not efficient.
// - Connecttion pushes incoming data to the `Stream` via channel, not buffer
// - Stream must be closed explictly Since garbage collect is not implemented.
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
    future::{select, Either},
    prelude::*,
    select,
    stream::FusedStream,
};
use futures_timer::Delay;

use crate::{
    error::ConnectionError,
    frame::{io, Frame, FrameDecodeError, StreamID, Tag},
    pause::Pausable,
};
use control::Control;
use libp2prs_traits::{SplitEx, SplittableReadWrite};
use nohash_hasher::IntMap;
use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::time::Duration;
use stream::{State, Stream};

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
    SendFrame(Frame, oneshot::Sender<()>),
    /// Close a stream.
    CloseStream(Frame, oneshot::Sender<()>),
    /// Reset a stream.
    ResetStream(Frame, oneshot::Sender<()>),
}

/// The connection identifier.
///
/// Randomly generated, this is mainly intended to improve log output.
#[derive(Clone, Copy)]
pub struct Id(u32);

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
        matches!(self, Shutdown::NotStarted)
    }

    fn is_in_progress(&self) -> bool {
        matches!(self, Shutdown::InProgress(_))
    }

    fn is_complete(&self) -> bool {
        matches!(self, Shutdown::Complete)
    }
}

/// Arbitrary limit of our internal command channels.
///
/// Since each `mpsc::Sender` gets a guaranteed slot in a channel the
/// actual upper bound is this value + number of clones.
const MAX_COMMAND_BACKLOG: usize = 32;
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(5);

type Result<T> = std::result::Result<T, ConnectionError>;

pub struct Connection<T: SplitEx> {
    id: Id,
    reader: Pin<Box<dyn FusedStream<Item = std::result::Result<Frame, FrameDecodeError>> + Send>>,
    writer: io::IO<T::Writer>,
    is_closed: bool,
    shutdown: Shutdown,
    next_stream_id: u32,
    streams: IntMap<StreamID, mpsc::Sender<Vec<u8>>>,
    streams_stat: IntMap<StreamID, State>,
    stream_sender: mpsc::Sender<StreamCommand>,
    stream_receiver: mpsc::Receiver<StreamCommand>,
    control_sender: mpsc::Sender<ControlCommand>,
    control_receiver: Pausable<mpsc::Receiver<ControlCommand>>,
    waiting_stream_sender: Option<oneshot::Sender<Result<stream::Stream>>>,
    pending_streams: VecDeque<stream::Stream>,
}

impl<T: SplittableReadWrite> Connection<T> {
    /// Create a new `Connection` from the given I/O resource.
    pub fn new(socket: T) -> Self {
        let id = Id::random();
        log::debug!("new connection: {}", id);

        let (reader, writer) = socket.split();
        let reader = io::IO::new(id, reader);
        let reader = futures::stream::unfold(reader, |mut io| async { Some((io.recv_frame().await, io)) });
        let reader = Box::pin(reader);

        let writer = io::IO::new(id, writer);
        let (stream_sender, stream_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
        let (control_sender, control_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);

        Connection {
            id,
            reader,
            writer,
            is_closed: false,
            next_stream_id: 0,
            shutdown: Shutdown::NotStarted,
            streams: IntMap::default(),
            streams_stat: IntMap::default(),
            stream_sender,
            stream_receiver,
            control_sender,
            control_receiver: Pausable::new(control_receiver),
            waiting_stream_sender: None,
            pending_streams: VecDeque::default(),
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
        log::info!("{}: error exit, {:?}", self.id, result);

        self.is_closed = true;

        if let Some(sender) = self.waiting_stream_sender.take() {
            sender.send(Err(ConnectionError::Closed)).expect("send err");
        }

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
            while self.stream_receiver.next().await.is_some() {
                while let Some(cmd) = self.stream_receiver.next().await {
                    match cmd {
                        StreamCommand::SendFrame(_, reply) => {
                            let _ = reply.send(());
                        }
                        StreamCommand::CloseStream(_, reply) => {
                            let _ = reply.send(());
                        }
                        StreamCommand::ResetStream(_, reply) => {
                            let _ = reply.send(());
                        }
                    }
                    // drop it
                    log::debug!("drop stream receiver frame");
                }
            }
        }

        result
    }

    /// This is called from `Connection::next_stream` instead of being a
    /// public method itself in order to guarantee proper closing in
    /// case of an error or at EOF.
    pub async fn handle_coming(&mut self) -> Result<()> {
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
        }
    }

    /// Process the result of reading from the socket.
    ///
    /// Unless `frame` is `Ok(()))` we will assume the connection got closed
    /// and return a corresponding error, which terminates the connection.
    /// Otherwise we process the frame
    async fn on_frame(&mut self, frame: Frame) -> Result<()> {
        log::trace!("{}: received: {}", self.id, frame.header());
        match frame.header().tag() {
            Tag::NewStream => {
                let stream_id = frame.header().stream_id();
                if self.streams_stat.contains_key(&stream_id) {
                    log::error!("received NewStream message for existing stream: {}", stream_id);
                    return Err(ConnectionError::Io(std::io::ErrorKind::InvalidData.into()));
                }

                let (stream_sender, stream_receiver) = mpsc::channel(MAX_COMMAND_BACKLOG);
                self.streams.insert(stream_id, stream_sender);
                self.streams_stat.insert(stream_id, State::Open);

                let stream = Stream::new(stream_id, self.id, self.stream_sender.clone(), stream_receiver);

                log::debug!("{}: new inbound {} of {}", self.id, stream, self);
                if let Some(sender) = self.waiting_stream_sender.take() {
                    sender.send(Ok(stream)).expect("send err");
                } else {
                    self.pending_streams.push_back(stream);
                }
            }
            Tag::Message => {
                let stream_id = frame.header().stream_id();
                if let Some(stat) = self.streams_stat.get(&stream_id) {
                    // if remote had close stream, ingore this stream's frame
                    if *stat == State::RecvClosed {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }

                let mut reset = false;
                let mut dropped = false;
                // If stream is closed, ignore frame
                if let Some(sender) = self.streams.get_mut(&stream_id) {
                    if !sender.is_closed() {
                        let sender = sender.send(frame.body());
                        if send_channel_timeout(sender, RECEIVE_TIMEOUT).await.is_err() {
                            // reset stream
                            log::debug!("stream {} send timeout, Reset it", stream_id);
                            reset = true;
                            // info.sender.close().await;
                            let frame = Frame::reset_frame(stream_id);
                            self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;
                        }
                    } else {
                        dropped = true;
                    }
                }
                // If the stream is dropped, remove sender from streams
                if dropped {
                    self.streams.remove(&stream_id);
                }
                if reset {
                    self.streams.remove(&stream_id);
                    self.streams_stat.remove(&stream_id);
                }
            }
            Tag::Close => {
                let stream_id = frame.header().stream_id();
                log::debug!("{}: remote close stream {} of {}", self.id, stream_id, self);
                self.streams.remove(&stream_id);
                // flag to remove stat from streams_stat
                let mut rm = false;
                if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
                    if *stat == State::SendClosed {
                        rm = true;
                    } else {
                        *stat = State::RecvClosed;
                    }
                }
                // If stream is completely closed, remove it
                if rm {
                    self.streams_stat.remove(&stream_id);
                }
            }
            Tag::Reset => {
                let stream_id = frame.header().stream_id();
                log::trace!("{}: remote reset stream {} of {}", self.id, stream_id, self);
                self.streams_stat.remove(&stream_id);
                self.streams.remove(&stream_id);
            }
        };

        Ok(())
    }

    /// Process a command from one of our `Stream`s.
    async fn on_stream_command(&mut self, cmd: Option<StreamCommand>) -> Result<()> {
        match cmd {
            Some(StreamCommand::SendFrame(frame, reply)) => {
                let stream_id = frame.stream_id();
                if let Some(stat) = self.streams_stat.get(&stream_id) {
                    if stat.can_write() {
                        log::trace!("{}: sending: {}", self.id, frame.header());
                        self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

                        let _ = reply.send(());
                    } else {
                        log::trace!("{}: stream {} have been removed", self.id, stream_id);
                        drop(reply);
                    }
                }
            }
            Some(StreamCommand::CloseStream(frame, reply)) => {
                let stream_id = frame.stream_id();
                log::debug!("{}: closing stream {} of {}", self.id, stream_id, self);
                // flag to remove stat from streams_stat
                let mut rm = false;
                if let Some(stat) = self.streams_stat.get_mut(&stream_id) {
                    if stat.can_write() {
                        // send close frame
                        self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

                        if *stat == State::RecvClosed {
                            rm = true;
                        } else {
                            *stat = State::SendClosed;
                        }
                    }
                }
                // If stream is completely closed, remove it
                if rm {
                    self.streams_stat.remove(&stream_id);
                }
                let _ = reply.send(());
            }
            Some(StreamCommand::ResetStream(frame, reply)) => {
                let stream_id = frame.stream_id();
                log::debug!("{}: reset stream {} of {}", self.id, stream_id, self);
                if self.streams_stat.contains_key(&stream_id) {
                    // step1: send close frame
                    self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

                    // step2: remove stream
                    self.streams_stat.remove(&stream_id);
                    self.streams.remove(&stream_id);
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
                self.streams_stat.insert(stream_id, State::Open);

                log::debug!("{}: new outbound {} of {}", self.id, stream_id, self);

                // send to peer with new stream frame
                let body = format!("{}", stream_id.val());
                let frame = Frame::new_stream_frame(stream_id, body.as_bytes());
                self.writer.send_frame(&frame).await.or(Err(ConnectionError::Closed))?;

                let stream = Stream::new(stream_id, self.id, self.stream_sender.clone(), stream_receiver);
                reply.send(Ok(stream)).expect("send err");
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

async fn send_channel_timeout<F>(future: F, timeout: Duration) -> std::io::Result<()>
where
    F: Future + Unpin,
{
    let output = select(future, Delay::new(timeout)).await;
    match output {
        Either::Left((_, _)) => Ok(()),
        Either::Right(_) => Err(std::io::ErrorKind::TimedOut.into()),
    }
}

impl<T: SplitEx> fmt::Display for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Connection {} (streams {}))", self.id, self.streams.len())
    }
}

impl<T: SplitEx> Connection<T> {
    // next_stream_id is only used to get stream id when open stream
    fn next_stream_id(&mut self) -> Result<StreamID> {
        let proposed = StreamID::new(self.next_stream_id, true);
        self.next_stream_id = self.next_stream_id.checked_add(1).ok_or(ConnectionError::NoMoreStreamIds)?;

        Ok(proposed)
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
}
