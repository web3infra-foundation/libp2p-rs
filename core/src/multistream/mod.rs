#![feature(fn_traits)]
// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! # Multistream-select Protocol Negotiation
//!
//! This crate implements the `multistream-select` protocol, which is the protocol
//! used by libp2p to negotiate which application-layer protocol to use with the
//! remote on a connection or substream.
//!
//! > **Note**: This crate is used primarily by core components of *libp2p* and it
//! > is usually not used directly on its own.
//!
//! ## Roles
//!
//! Two peers using the multistream-select negotiation protocol on an I/O stream
//! are distinguished by their role as a _dialer_ (or _initiator_) or as a _listener_
//! (or _responder_). Thereby the dialer plays the active part, driving the protocol,
//! whereas the listener reacts to the messages received.
//!
//! The dialer has two options: it can either pick a protocol from the complete list
//! of protocols that the listener supports, or it can directly suggest a protocol.
//! Either way, a selected protocol is sent to the listener who can either accept (by
//! echoing the same protocol) or reject (by responding with a message stating
//! "not available"). If a suggested protocol is not available, the dialer may
//! suggest another protocol. This process continues until a protocol is agreed upon,
//! yielding a [`Negotiated`](self::Negotiated) stream, or the dialer has run out of
//! alternatives.
//!
//! See [`dialer_select_proto`](self::dialer_select_proto) and
//! [`listener_select_proto`](self::listener_select_proto).
//!
//! ## [`Negotiated`](self::Negotiated)
//!
//! When a dialer or listener participating in a negotiation settles
//! on a protocol to use, the [`DialerSelectFuture`] respectively
//! [`ListenerSelectFuture`] yields a [`Negotiated`](self::Negotiated)
//! I/O stream.
//!
//! Notably, when a `DialerSelectFuture` resolves to a `Negotiated`, it may not yet
//! have written the last negotiation message to the underlying I/O stream and may
//! still be expecting confirmation for that protocol, despite having settled on
//! a protocol to use.
//!
//! Similarly, when a `ListenerSelectFuture` resolves to a `Negotiated`, it may not
//! yet have sent the last negotiation message despite having settled on a protocol
//! proposed by the dialer that it supports.
//!
//! This behaviour allows both the dialer and the listener to send data
//! relating to the negotiated protocol together with the last negotiation
//! message(s), which, in the case of the dialer only supporting a single
//! protocol, results in 0-RTT negotiation. Note, however, that a dialer
//! that performs multiple 0-RTT negotiations in sequence for different
//! protocols layered on top of each other may trigger undesirable behaviour
//! for a listener not supporting one of the intermediate protocols.
//! See [`dialer_select_proto`](self::dialer_select_proto).
//!
//! ## Examples
//!
//! For a dialer:
//!
//! ```no_run
//! # fn main() {
//! use async_std::net::TcpStream;
//! use self::{Version};
//! use futures::prelude::*;
//!
//! async_std::task::block_on(async move {
//!     let socket = TcpStream::connect("127.0.0.1:10333").await.unwrap();
//!
//!     let protos = vec![b"/echo/1.0.0", b"/echo/2.5.0"];
//!     let (protocol, _io) = dialer_select_proto(socket, protos, Version::V1).await.unwrap();
//!
//!     println!("Negotiated protocol: {:?}", protocol);
//!     // You can now use `_io` to communicate with the remote.
//! });
//! # }
//! ```
//!

// mod dialer_select;
mod length_delimited;
// mod listener_select;
mod protocol;
mod negotiator;
pub mod muxer;
mod upgrade;
mod tests;

pub use self::negotiator::{Negotiator, NegotiationError};
pub use self::protocol::{ProtocolError, Version};
// pub use self::dialer_select::{dialer_select_proto, DialerSelectFuture};
// pub use self::listener_select::{listener_select_proto, ListenerSelectFuture};

pub(self) use libp2p_traits::Read2 as ReadEx;
pub(self) use libp2p_traits::Write2 as WriteEx;

#[cfg(test)]
pub(self) use tests::Memory;

/*
use std::io;
use async_trait::async_trait;

#[async_trait]
pub trait ReadEx {
    async fn async_read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    async fn async_read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut buf_piece = buf;
        while !buf_piece.is_empty() {
            let n = self.async_read(buf_piece).await?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }

            let (_, rest) = buf_piece.split_at_mut(n);
            buf_piece = rest;
        }
        Ok(())
    }
}

#[async_trait]
pub trait WriteEx {
    async fn async_write(&mut self, buf: &[u8]) -> io::Result<usize>;

    async fn async_flush(&mut self) -> io::Result<()>;

    async fn async_close(&mut self) -> io::Result<()>;

    async fn async_write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut buf_piece = buf;
        while !buf_piece.is_empty() {
            let n = self.async_write(buf).await?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            let (_, rest) = buf_piece.split_at(n);
            buf_piece = rest;
        }
        Ok(())
    }
}

use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[async_trait]
impl<R: AsyncRead + Send + Unpin> ReadEx for R {
    async fn async_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf).await
    }
}

#[async_trait]
impl<W: AsyncWrite + Send + Unpin> WriteEx for W {
    async fn async_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write(buf).await
    }

    async fn async_flush(&mut self) -> io::Result<()> {
        self.flush().await
    }

    async fn async_close(&mut self) -> io::Result<()> {
        self.close().await
    }
}
 */