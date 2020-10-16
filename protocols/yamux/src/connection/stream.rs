// Copyright (c) 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use std::{fmt, io, sync::{atomic::{AtomicUsize, AtomicU32, Ordering}, Arc}, };
use async_trait::async_trait;

use futures::prelude::*;
use futures::{channel::{mpsc}, SinkExt};
use futures::lock::{Mutex, MutexGuard};

use libp2p_traits::{ReadEx, WriteEx};

use crate::{
    chunks::Chunks,
    connection::{self, StreamCommand},
    ConnectionError,
    Config,
    frame::{Frame, header::{HasAck, HasSyn, Header, StreamId}},
    WindowUpdateMode,
};

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

const STATE_OPEN: usize = 1;
const STATE_SEND_CLOSED: usize = 2;
const STATE_RECV_CLOSED: usize = 3;
const STATE_CLOSED: usize = 4;

impl State {
    pub(crate) fn from_usize(state: usize) -> Self {
        match state {
            STATE_OPEN => State::Open,
            STATE_SEND_CLOSED => State::SendClosed,
            STATE_RECV_CLOSED => State::RecvClosed,
            STATE_CLOSED => State::Closed,
            _ => { panic!("Unknown state") }
        }
    }

    pub(crate) fn to_usize(self) -> usize {
        match self {
            State::Open => STATE_OPEN,
            State::SendClosed => STATE_SEND_CLOSED,
            State::RecvClosed => STATE_RECV_CLOSED,
            State::Closed => STATE_CLOSED,
        }
    }

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
#[derive(Clone)]
pub struct Stream {
    id: StreamId,
    conn: connection::Id,
    config: Arc<Config>,
    sender: mpsc::UnboundedSender<StreamCommand>,
    flag: Flag,
    shared: Arc<Shared>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Stream")
            .field("id", &self.id.val())
            .field("connection", &self.conn)
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
        sender: mpsc::UnboundedSender<StreamCommand>,
    ) -> Self {
        Stream {
            id,
            conn,
            config,
            sender,
            flag: Flag::None,
            shared: Arc::new(Shared::new(id, window, credit)),
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
    pub(crate) fn state(&self) -> State {
        self.shared().state()
    }

    pub(crate) fn strong_count(&self) -> usize {
        Arc::strong_count(&self.shared)
    }

    pub(crate) fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub(crate) fn clone(&self) -> Self {
        Stream {
            id: self.id,
            conn: self.conn,
            config: self.config.clone(),
            sender: self.sender.clone(),
            flag: self.flag,
            shared: self.shared.clone(),
        }
    }

    async fn read_stream(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if !self.config.read_after_close && self.sender.is_closed() {
                // TBD: return err???
                return Ok(0);
            }

            // Copy data from stream buffer.
            let shared = self.shared();
            let mut n = 0;
            {
                let mut buffer = shared.buffer.lock().await;
                while let Some(chunk) = buffer.front_mut() {
                    if chunk.is_empty() {
                        buffer.pop();
                        continue
                    }
                    let k = std::cmp::min(chunk.len(), buf.len() - n);
                    (&mut buf[n .. n + k]).copy_from_slice(&chunk.as_ref()[.. k]);
                    n += k;
                    chunk.advance(k);
                    if n == buf.len() {
                        break
                    }
                }
            }
            if n > 0 {
                log::trace!("{}/{}: read {} bytes", self.conn, self.id, n);
                return Ok(n)
            }

            // Buffer is empty, let's check if we can expect to read more data.
            if !shared.state().can_read() {
                log::info!("3 {}/{}: eof", self.conn, self.id);
                // return Err(io::ErrorKind::BrokenPipe.into()); // stream has been reset
                return Ok(0);
            }

            if self.config.window_update_mode == WindowUpdateMode::OnRead
               && shared.window() == 0 {

                let mut frame = Frame::window_update(self.id, self.config.receive_window);
                self.add_flag(frame.header_mut());
                let cmd = StreamCommand::SendFrame(frame.right());
                self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;
                self.shared.window.store(self.config.receive_window, Ordering::SeqCst);
            }

            log::trace!("waiting stream({}) recv", self.id());
            shared.wait_recv().await;
            log::trace!("stream({}) recv wakeup", self.id());
        }
    }

    async fn write_stream(&mut self, buf: &[u8]) -> io::Result<usize> {

        let shared = self.shared();
        if !shared.state().can_write() {
            log::debug!("{}/{}: can no longer write", self.conn, self.id);
            return Err(self.write_zero_err());
        }

        while shared.credit() == 0 {
            log::info!("{}/{}: no more credit left", self.conn, self.id);
            shared.wait_send().await;
        }

        let body = {
            let k = std::cmp::min(shared.credit() as usize, buf.len());
            shared.credit.store(shared.credit().saturating_sub(k as u32), Ordering::SeqCst);
            Vec::from(&buf[.. k])
        };

        let n = body.len();
        let mut frame = Frame::data(self.id, body).expect("body <= u32::MAX").left();
        self.add_flag(frame.header_mut());
        log::trace!("{}/{}: write {} bytes", self.conn, self.id, n);
        let cmd = StreamCommand::SendFrame(frame);
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;
        Ok(n)
    }

    async fn close_stream(&mut self) -> io::Result<()> {
        if self.state() == State::Closed {
            return Ok(());
        }

        let ack = if self.flag == Flag::Ack {
            self.flag = Flag::None;
            true
        } else {
            false
        };

        let cmd = StreamCommand::CloseStream { id: self.id, ack };
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;

        self.shared().update_state(self.conn, self.id, State::SendClosed);

        log::trace!("{}/{}: close", self.conn, self.id);
        Ok(())
    }

    fn write_zero_err(&self) -> io::Error {
        let msg = format!("{}/{}: connection is closed", self.conn, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }

    /// Set ACK or SYN flag if necessary.
    fn add_flag<T: HasAck + HasSyn>(&mut self, header: &mut Header<T>) {
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

    /*
    #[allow(dead_code)]
    pub(crate) async fn gen_window_update(&mut self) -> Option<Frame<WindowUpdate>> {
        let max = self.config.receive_window;
        let blen = self.shared.buffer.lock().await.len().expect("buffer len") as u32;
        let window = self.shared.window();

        let delta = max - blen - window;

        if delta < (max / 2) && self.flag == Flag::None {
            return None
        }

        self.shared.window.store(window + delta, Ordering::SeqCst);

        let mut frame = Frame::window_update(self.id, delta);
        self.add_flag(frame.header_mut());

        Some(frame)
    }
     */
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
    stream_id: StreamId,
    state: AtomicUsize,
    pub(crate) window: AtomicU32,
    pub(crate) credit: AtomicU32,

    send_notify: (mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>),
    recv_notify: (mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>),

    buffer: Mutex<Chunks>,
}

impl Shared {
    fn new(stream_id: StreamId, window: u32, credit: u32) -> Self {
        let (send_tx, send_rx) = mpsc::channel(1);
        let (recv_tx, recv_rx) = mpsc::channel(1);
        Shared {
            stream_id,
            state: AtomicUsize::new(State::Open.to_usize()),
            window: AtomicU32::new(window),
            credit: AtomicU32::new(credit),
            send_notify: (send_tx, Mutex::new(send_rx)),
            recv_notify: (recv_tx, Mutex::new(recv_rx)),
            buffer: Mutex::new(Chunks::new()),
        }
    }

    pub(crate) fn state(&self) -> State {
        State::from_usize(self.state.load(Ordering::SeqCst))
    }

    pub(crate) fn window(&self) -> u32 {
        self.window.load(Ordering::SeqCst)
    }

    pub(crate) fn sub_window(&self, val: u32) {
        self.window.store(self.window().saturating_sub(val), Ordering::SeqCst);
    }

    pub(crate) fn update_window(&self, val: u32) {
        self.window.store(val, Ordering::SeqCst);
    }

    pub(crate) fn credit(&self) -> u32 {
        self.credit.load(Ordering::SeqCst)
    }

    pub(crate) fn add_credit(&self, val: u32) {
        self.credit.fetch_add(val, Ordering::SeqCst);
    }

    pub(crate) fn notify_send(&self) -> Result<(), ConnectionError>{
        log::info!("Stream({}) notify stream send", self.stream_id);
        Self::notify(self.send_notify.0.clone())
    }

    pub(crate) fn notify_recv(&self) -> Result<(), ConnectionError>{
        log::info!("Stream({}) notify stream recv", self.stream_id);
        Self::notify(self.recv_notify.0.clone())
    }

    fn notify(mut tx: mpsc::Sender<()>) -> Result<(), ConnectionError> {
        match tx.try_send(()) {
            Ok(_) => Ok(()),
            Err(e) => {
                let e = e.into_send_error();
                if e.is_full() {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn wait_send(&self) {
        self.send_notify.1.lock().await.next().await;
    }

    async fn wait_recv(&self) {
        self.recv_notify.1.lock().await.next().await;
    }

    /// Update the stream state and return the state before it was updated.
    pub(crate) fn update_state(&self, cid: connection::Id, sid: StreamId, next: State) -> State {
        let current = self.state();

        log::trace!("update state current({:?}) -> next({:?})", current, next);

        use State::*;

        let to_store = match (current, next) {
            (Closed, _) => None,
            (Open, _) => Some(next),
            (RecvClosed, Closed) => Some(Closed),
            (RecvClosed, Open) => None,
            (RecvClosed, RecvClosed) => None,
            (RecvClosed, SendClosed) => Some(Closed),
            (SendClosed, Closed) => Some(Closed),
            (SendClosed, Open) => None,
            (SendClosed, RecvClosed) => Some(Closed),
            (SendClosed, SendClosed) => None,
        };

        if let Some(next) = to_store {
            self.state.store(next.to_usize(), Ordering::SeqCst);
        }

        log::trace!("{}/{}: update state: ({:?} {:?} {:?})", cid, sid, current, next, self.state);

        current // Return the previous stream state for informational purposes.
    }

    pub(crate) async fn buffer(&self) -> MutexGuard<'_, Chunks> {
        self.buffer.lock().await
    }
}

#[async_trait]
impl ReadEx for Stream {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_stream(buf).await
    }
}

#[async_trait]
impl WriteEx for Stream {
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
