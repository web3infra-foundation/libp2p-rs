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

use crate::{secio::error::SecioError, SessionId};
use libp2prs_multiaddr::Multiaddr;
use std::io::Error as IOError;
use thiserror::Error;

#[derive(Error, Debug)]
/// Transport Error
pub enum TransportErrorKind {
    /// IO error
    #[error("transport io error: `{0:?}`")]
    Io(IOError),
    /// Protocol not support
    #[error("multiaddr `{0:?}` is not supported")]
    NotSupported(Multiaddr),
    /// Dns resolver error
    #[error("can not resolve `{0:?}`, io error: `{1:?}`")]
    DNSResolverError(Multiaddr, IOError),
}

#[derive(Error, Debug)]
/// Protocol handle error
pub enum ProtocolHandleErrorKind {
    /// protocol handle block, may be user's protocol handle implementation problem
    #[error("protocol handle block, session id: `{0:?}`")]
    Block(Option<SessionId>),
    /// protocol handle abnormally closed, may be user's protocol handle implementation problem
    #[error("protocol handle abnormally closed, session id: `{0:?}`")]
    AbnormallyClosed(Option<SessionId>),
}

#[derive(Error, Debug)]
/// Detail error kind when dial remote error
pub enum DialerErrorKind {
    /// IO error
    #[error("dialler io error: `{0:?}`")]
    IoError(IOError),
    /// When dial remote, peer id does not match
    #[error("peer id not match")]
    PeerIdNotMatch,
    /// Connected to the connected peer
    #[error("repeated connection, sessio id: `{0:?}`")]
    RepeatedConnection(SessionId),
    /// Handshake error
    #[error("handshake error: `{0:?}`")]
    HandshakeError(HandshakeErrorKind),
    /// Transport error
    #[error("transport error: `{0:?}`")]
    TransportError(TransportErrorKind),
}

#[derive(Error, Debug)]
/// Handshake error
pub enum HandshakeErrorKind {
    /// Handshake timeout error
    #[error("timeout error: `{0:?}`")]
    Timeout(String),
    /// Secio error
    #[error("secio error: `{0:?}`")]
    SecioError(SecioError),
}

#[derive(Error, Debug)]
/// Listener error kind when dial remote error
pub enum ListenErrorKind {
    /// IO error
    #[error("listen io error: `{0:?}`")]
    IoError(IOError),
    /// Connected to the connected peer
    #[error("repeated connection, sessio id: `{0:?}`")]
    RepeatedConnection(SessionId),
    /// Transport error
    #[error("transport error: `{0:?}`")]
    TransportError(TransportErrorKind),
}

#[derive(Error, Debug)]
/// Send error kind when send service task
pub enum SendErrorKind {
    /// Sending failed because a pipe was closed.
    #[error("broken pipe")]
    BrokenPipe,
    /// The operation needs to block to complete, but the blocking operation was requested to not occur.
    #[error("would block")]
    WouldBlock,
}
