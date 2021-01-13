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

use crate::{
    connection::{Id, StreamCommand},
    frame::{header::StreamId, Frame},
    Config,
};
use async_trait::async_trait;
use bytes::{Buf, BufMut};
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2prs_traits::{ReadEx, WriteEx};
use std::sync::Arc;
use std::{fmt, io};

/// The state of a Yamux stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Open bidirectionally.
    Open,
    /// Open for incoming messages.
    SendClosed,
    /// Open for outgoing messages.
    RecvClosed,
}

impl State {
    /// Can we receive messages over this stream?
    pub fn can_read(self) -> bool {
        self != State::RecvClosed
    }

    /// Can we send messages over this stream?
    pub fn can_write(self) -> bool {
        self != State::SendClosed
    }
}

#[derive(Debug)]
pub(crate) struct StreamStat {
    state: State,
    flag: Flag,
    pub(crate) window: u32,
    pub(crate) credit: usize,
}

impl StreamStat {
    pub(crate) fn new(window: u32, credit: usize) -> Self {
        StreamStat {
            state: State::Open,
            flag: Flag::None,
            window,
            credit,
        }
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }

    /// Update the stream state and return the state before it was updated.
    pub(crate) fn update_state(&mut self, cid: Id, sid: StreamId, next: State) -> State {
        let current = self.state;
        self.state = next;

        log::trace!("{}/{}: update state: ({:?} {:?} {:?})", cid, sid, current, next, self.state);
        current // Return the previous stream state for informational purposes.
    }

    pub fn get_flag(&self) -> Flag {
        self.flag
    }

    pub fn set_flag(&mut self, flag: Flag) {
        self.flag = flag
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

pub struct Stream {
    id: StreamId,
    conn_id: Id,
    config: Arc<Config>,
    read_buffer: bytes::BytesMut,
    sender: mpsc::UnboundedSender<StreamCommand>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id)
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Stream {}/{})", self.conn_id, self.id)
    }
}

impl Clone for Stream {
    /// impl [`Clone`] trait
    fn clone(&self) -> Self {
        Stream {
            id: self.id,
            conn_id: self.conn_id,
            config: self.config.clone(),
            read_buffer: Default::default(),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
        }
    }
}

impl Stream {
    pub(crate) fn new(
        id: StreamId,
        conn_id: Id,
        config: Arc<Config>,
        sender: mpsc::UnboundedSender<StreamCommand>,
        receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Self {
        Stream {
            id,
            conn_id,
            config,
            read_buffer: Default::default(),
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Get this stream's identifier.
    pub fn id(&self) -> u32 {
        self.id.val()
    }

    /// read data from receiver. If data is not drain, store in inner buffer
    pub(crate) async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.config.read_after_close && self.sender.is_closed() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        if self.read_buffer.has_remaining() {
            let len = std::cmp::min(self.read_buffer.remaining(), buf.len());
            buf[..len].copy_from_slice(&self.read_buffer[..len]);
            self.read_buffer.advance(len);
            return Ok(len);
        }

        let mut receiver = self.receiver.lock().await;
        if let Some(data) = receiver.next().await {
            let dlen = data.len();
            let len = std::cmp::min(data.len(), buf.len());
            buf[..len].copy_from_slice(&data[..len]);

            if len < dlen {
                self.read_buffer.reserve(dlen - len);
                self.read_buffer.put(&data[len..dlen]);
            }
            return Ok(len);
        }

        Err(io::ErrorKind::UnexpectedEof.into())
    }

    /// write stream, the max size is config.max_message_size
    /// If stream has closed, return err
    pub(crate) async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.sender.is_closed() {
            return Err(self.closed_err());
        }

        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }

        let (tx, rx) = oneshot::channel();
        let n = std::cmp::min(self.config.max_message_size, len);

        let frame = Frame::data(self.id, buf[..n].to_vec()).expect("body <= u32::MAX");
        let cmd = StreamCommand::SendFrame(frame, tx);
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;

        let n = rx.await.map_err(|_| self.closed_err())?;
        log::debug!("{}/{}: write {} bytes", self.conn_id, self.id, n);

        Ok(n)
    }

    /// close stream, sender will be closed and state will turn to SendClosed
    /// If stream has closed, return ()
    async fn close(&mut self) -> io::Result<()> {
        if self.sender.is_closed() {
            return Ok(());
        }

        let (tx, rx) = oneshot::channel();
        let cmd = StreamCommand::CloseStream(self.id, tx);
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;
        rx.await.map_err(|_| self.closed_err())?;

        self.sender.close().await.expect("close err");

        Ok(())
    }

    /// reset stream, sender will be closed and state will turn to Closed
    /// If stream has reset, return ()
    pub async fn reset(&mut self) -> io::Result<()> {
        if self.sender.is_closed() {
            return Ok(());
        }

        let (tx, rx) = oneshot::channel();
        let cmd = StreamCommand::ResetStream(self.id, tx);
        self.sender.send(cmd).await.map_err(|_| self.write_zero_err())?;
        rx.await.map_err(|_| self.closed_err())?;

        self.sender.close().await.expect("close err");

        Ok(())
    }

    /// connection is closed
    fn write_zero_err(&self) -> io::Error {
        let msg = format!("{}/{}: connection is closed", self.conn_id, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }

    /// stream is closed or reset
    fn closed_err(&self) -> io::Error {
        let msg = format!("{}/{}: stream is closed / reset", self.conn_id, self.id);
        io::Error::new(io::ErrorKind::WriteZero, msg)
    }
}

#[async_trait]
impl ReadEx for Stream {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf).await
    }
}

#[async_trait]
impl WriteEx for Stream {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        Ok(())
    }

    async fn close2(&mut self) -> io::Result<()> {
        self.close().await
    }
}
