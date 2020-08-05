// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use crate::{
    chunks::Chunks,
    connection::{self, StreamCommand},
    frame::{
        header::{Data, Header, StreamId, WindowUpdate},
        Frame,
    },
    Config, WindowUpdateMode,
};
//use futures::lock::{Mutex, MutexGuard};
use futures::prelude::*;
use futures::{channel::mpsc, future::Either, SinkExt};
use std::{
    fmt, io,
    sync::Arc,
    task::{Poll, Waker},
};

use async_trait::async_trait;
use futures::task::AtomicWaker;
use futures::lock::{Mutex, MutexGuard};

use libp2p_traits::{Read2, Write2};

/// The state of a Yamux stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Open bidirectionally.
    Open,
    /// Open for incoming messages.
    SendClosed,
    /// Open for outgoing messages.
    RecvClosed,
    /// Closed (terminal state).
    Closed,
}

impl State {
    /// Can we receive messages over this stream?
    pub fn can_read(self) -> bool {
        if let State::RecvClosed | State::Closed = self {
            false
        } else {
            true
        }
    }

    /// Can we send messages over this stream?
    pub fn can_write(self) -> bool {
        if let State::SendClosed | State::Closed = self {
            false
        } else {
            true
        }
    }
}

/// Indicate if a flag still needs to be set on an outbound header.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Flag {
    /// No flag needs to be set.
    None,
    /// The stream was opened lazily, so set the initial SYN flag.
    Syn,
    /// The stream still needs acknowledgement, so set the ACK flag.
    Ack,
}

/// A multiplexed Yamux stream.
///
/// Streams are created either outbound via [`crate::Control::open_stream`]
/// or inbound via [`crate::Connection::next_stream`].
///
/// `Stream` implements [`AsyncRead`] and [`AsyncWrite`] and also
/// [`futures::stream::Stream`].
pub struct Stream {
    id: StreamId,
    conn: connection::Id,
    config: Arc<Config>,
    sender: mpsc::Sender<StreamCommand>,
    pending: Option<Frame<WindowUpdate>>,
    flag: Flag,
    shared: Arc<Mutex<Shared>>,

    pub(crate) reader: Arc<AtomicWaker>,
    //pub(crate) writer: AtomicWaker,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Stream")
            .field("id", &self.id.val())
            .field("connection", &self.conn)
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn, self.id.val())
    }
}

impl Stream {
    pub(crate) fn new(
        id: StreamId,
        conn: connection::Id,
        config: Arc<Config>,
        window: u32,
        credit: u32,
        sender: mpsc::Sender<StreamCommand>,
    ) -> Self {
        Stream {
            id,
            conn,
            config,
            sender,
            pending: None,
            flag: Flag::None,
            shared: Arc::new(Mutex::new(Shared::new(window, credit))),
            reader: Arc::new(Default::default()),
        }
    }

    /// Get this stream's identifier.
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Set the flag that should be set on the next outbound frame header.
    pub(crate) fn set_flag(&mut self, flag: Flag) {
        self.flag = flag
    }

    /// Get this stream's state.
    pub(crate) async fn state(&self) -> State {
        self.shared().await.state()
    }

    pub(crate) fn strong_count(&self) -> usize {
        Arc::strong_count(&self.shared)
    }

    pub(crate) async fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().await
    }

    pub(crate) fn clone(&self) -> Self {
        Stream {
            id: self.id,
            conn: self.conn,
            config: self.config.clone(),
            sender: self.sender.clone(),
            pending: None,
            flag: self.flag,
            shared: self.shared.clone(),
            reader: self.reader.clone(),
            //writer: None
        }
    }

    async fn read_stream(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.config.read_after_close && self.sender.is_closed() {
            // TBD: return err???
            return Ok(0);
        }

        // Copy data from stream buffer.
        let mut shared = self.shared().await;

        // Buffer is empty, let's check if we can expect to read more data.
        if !shared.state().can_read() {
            log::info!("{}/{}: eof", self.conn, self.id);
            return Err(io::ErrorKind::BrokenPipe.into()); // stream has been reset
        }
        drop(shared);

        log::debug!("{}/{}: reading", self.conn, self.id);

        future::poll_fn::<(), _>(|cx| {
            let fut = self.shared();
            futures::pin_mut!(fut);
            let mut shared = futures::ready!(fut.poll(cx));

            // Since we have no more data at this point, we want to be woken up
            // by the connection when more becomes available for us.
            // Note: shared will be dropped when it is out of scope
            if shared.buffer.len().unwrap() == 0 {
                log::debug!("{}/{}: empty buffer, go pending", self.conn, self.id);
                shared.reader = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;

        shared = self.shared().await;

        let mut n = 0;
        while let Some(chunk) = shared.buffer.front_mut() {
            if chunk.is_empty() {
                shared.buffer.pop();
                continue;
            }
            let k = std::cmp::min(chunk.len(), buf.len() - n);
            (&mut buf[n..n + k]).copy_from_slice(&chunk.as_ref()[..k]);
            n += k;
            chunk.advance(k);
            if n == buf.len() {
                break;
            }
        }

        log::trace!("{}/{}: read {} bytes", self.conn, self.id, n);

        // ok to send update window
        if self.config.window_update_mode == WindowUpdateMode::OnRead {
            let max = self.config.receive_window;
            let blen = shared.buffer.len().unwrap() as u32;
            let delta = max - blen - shared.window;

            // Determine the flags if any
            //flags := s.sendFlags()

            // Check if we can omit the update
            if delta < (max / 2) && self.flag == Flag::None {
                return Ok(n);
            }

            shared.window += delta;

            // release shared as soon as possible
            drop(shared);

            // At this point we know we have to send a window update to the remote.
            let frame = Frame::window_update(self.id, delta);
            let mut frame = frame.right();
            self.add_flag(frame.header_mut());
            let cmd = StreamCommand::SendFrame(frame);
            self.sender
                .send(cmd)
                .await
                .map_err(|_| self.write_zero_err())?;
        }
        Ok(n)
    }

    async fn write_stream(&mut self, buf: &[u8]) -> io::Result<usize> {
        let body = {
            let mut shared = self.shared().await;
            if !shared.state().can_write() {
                log::debug!("{}/{}: can no longer write", self.conn, self.id);
                return Err(self.write_zero_err());
            }
            drop(shared);

            future::poll_fn::<(), _>(|cx| {
                let fut = self.shared();
                futures::pin_mut!(fut);
                let mut shared = futures::ready!(fut.poll(cx));

                // No credit here, we want to be woken up
                // by the connection when more becomes available for us.
                // Note: shared will be dropped when it is out of scope
                if shared.credit == 0 {
                    log::debug!("{}/{}: no more credit left", self.conn, self.id);
                    shared.writer = Some(cx.waker().clone());
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;

            // re-gain the shared data
            shared = self.shared().await;
            let k = std::cmp::min(shared.credit as usize, buf.len());
            shared.credit = shared.credit.saturating_sub(k as u32);
            Vec::from(&buf[..k])
        };

        let n = body.len();
        let mut frame = Frame::data(self.id, body).expect("body <= u32::MAX").left();
        self.add_flag(frame.header_mut());
        log::trace!("{}/{}: write {} bytes", self.conn, self.id, n);
        let cmd = StreamCommand::SendFrame(frame);
        self.sender
            .send(cmd)
            .await
            .map_err(|_| self.write_zero_err())?;

        Ok(n)
    }

    async fn close_stream(&mut self) -> io::Result<()> {
        if self.state().await == State::Closed {
            return Ok(());
        }
        log::trace!("{}/{}: close", self.conn, self.id);

        let ack = if self.flag == Flag::Ack {
            self.flag = Flag::None;
            true
        } else {
            false
        };

        let cmd = StreamCommand::CloseStream { id: self.id, ack };
        self.sender
            .send(cmd)
            .await
            .map_err(|_| self.write_zero_err())?;

        self.shared()
            .await
            .update_state(self.conn, self.id, State::SendClosed);
        Ok(())
    }

    fn write_zero_err(&self) -> io::Error {
        let msg = format!("{}/{}: connection is closed", self.conn, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }

    /// Set ACK or SYN flag if necessary.
    fn add_flag(&mut self, header: &mut Header<Either<Data, WindowUpdate>>) {
        match self.flag {
            Flag::None => (),
            Flag::Syn => {
                header.syn();
                self.flag = Flag::None
            }
            Flag::Ack => {
                header.ack();
                self.flag = Flag::None
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        log::info!("drop stream {}", self.id);
        // uncomment it when we have async destructor support
        //self.close().await;
    }
}


#[derive(Debug)]
pub(crate) struct Shared {
    state: State,
    pub(crate) window: u32,
    pub(crate) credit: u32,
    pub(crate) buffer: Chunks,
    pub(crate) reader: Option<Waker>,
    pub(crate) writer: Option<Waker>,
}

impl Shared {
    fn new(window: u32, credit: u32) -> Self {
        Shared {
            state: State::Open,
            window,
            credit,
            buffer: Chunks::new(),
            reader: None,
            writer: None,
        }
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }

    /// Update the stream state and return the state before it was updated.
    pub(crate) fn update_state(
        &mut self,
        cid: connection::Id,
        sid: StreamId,
        next: State,
    ) -> State {
        use self::State::*;

        let current = self.state;

        match (current, next) {
            (Closed, _) => {}
            (Open, _) => self.state = next,
            (RecvClosed, Closed) => self.state = Closed,
            (RecvClosed, Open) => {}
            (RecvClosed, RecvClosed) => {}
            (RecvClosed, SendClosed) => self.state = Closed,
            (SendClosed, Closed) => self.state = Closed,
            (SendClosed, Open) => {}
            (SendClosed, RecvClosed) => self.state = Closed,
            (SendClosed, SendClosed) => {}
        }

        log::trace!(
            "{}/{}: update state: ({:?} {:?} {:?})",
            cid,
            sid,
            current,
            next,
            self.state
        );

        current // Return the previous stream state for informational purposes.
    }
}

#[async_trait]
impl Read2 for Stream {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_stream(buf).await
    }
}

#[async_trait]
impl Write2 for Stream {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_stream(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        Ok(())
    }

    async fn close2(&mut self) -> io::Result<()> {
        self.close_stream().await
    }
}