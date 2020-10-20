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

//! This module provides a `Sink` and `Stream` for length-delimited
//! Noise protocol messages in form of [`NoiseFramed`].

use crate::io::NoiseOutput;
use crate::{NoiseError, Protocol, PublicKey};
use bytes::{Bytes, BytesMut};
use libp2prs_core::identity;
use libp2prs_traits::{ReadEx, WriteEx};
use log::{debug, trace};
use std::{fmt, io};

/// Max. size of a noise message.
const MAX_NOISE_MSG_LEN: usize = 65535;
/// Space given to the encryption buffer to hold key material.
const EXTRA_ENCRYPT_SPACE: usize = 1024;
/// Max. length for Noise protocol message payloads.
pub const MAX_FRAME_LEN: usize = MAX_NOISE_MSG_LEN - EXTRA_ENCRYPT_SPACE;

static_assertions::const_assert! {
    MAX_FRAME_LEN + EXTRA_ENCRYPT_SPACE <= MAX_NOISE_MSG_LEN
}

/// A `NoiseFramed` is a `Sink` and `Stream` for length-delimited
/// Noise protocol messages.
///
/// `T` is the type of the underlying I/O resource and `S` the
/// type of the Noise session state.
pub struct NoiseFramed<T, S> {
    io: T,
    session: S,
    read_state: ReadState,
    write_state: WriteState,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    decrypt_buffer: BytesMut,
}

impl<T, S> fmt::Debug for NoiseFramed<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoiseFramed")
            .field("read_state", &self.read_state)
            .field("write_state", &self.write_state)
            .finish()
    }
}

impl<T> NoiseFramed<T, snow::HandshakeState> {
    /// Creates a nwe `NoiseFramed` for beginning a Noise protocol handshake.
    pub fn new(io: T, state: snow::HandshakeState) -> Self {
        NoiseFramed {
            io,
            session: state,
            read_state: ReadState::Ready,
            write_state: WriteState::Ready,
            read_buffer: Vec::new(),
            write_buffer: Vec::new(),
            decrypt_buffer: BytesMut::new(),
        }
    }

    /// Converts the `NoiseFramed` into a `NoiseOutput` encrypted data stream
    /// once the handshake is complete, including the static DH [`PublicKey`]
    /// of the remote, if received.
    ///
    /// If the underlying Noise protocol session state does not permit
    /// transitioning to transport mode because the handshake is incomplete,
    /// an error is returned. Similarly if the remote's static DH key, if
    /// present, cannot be parsed.
    pub fn into_transport<C>(self, keypair: identity::Keypair) -> Result<(Option<PublicKey<C>>, NoiseOutput<T>), NoiseError>
    where
        C: Protocol<C> + AsRef<[u8]>,
    {
        let dh_remote_pubkey = match self.session.get_remote_static() {
            None => None,
            Some(k) => match C::public_from_bytes(k) {
                Err(e) => return Err(e),
                Ok(dh_pk) => Some(dh_pk),
            },
        };
        match self.session.into_transport_mode() {
            Err(e) => Err(e.into()),
            Ok(s) => {
                let io = NoiseFramed {
                    session: s,
                    io: self.io,
                    read_state: ReadState::Ready,
                    write_state: WriteState::Ready,
                    read_buffer: self.read_buffer,
                    write_buffer: self.write_buffer,
                    decrypt_buffer: self.decrypt_buffer,
                };
                Ok((dh_remote_pubkey, NoiseOutput::new(io, keypair)))
            }
        }
    }
}

/// The states for reading Noise protocol frames.
#[derive(Debug)]
enum ReadState {
    /// Ready to read another frame.
    Ready,
    /// Reading frame length.
    ReadLen { buf: [u8; 2], off: usize },
    /// Reading frame data.
    ReadData { len: usize, off: usize },
    /// EOF has been reached (terminal state).
    ///
    /// The associated result signals if the EOF was unexpected or not.
    Eof(Result<(), ()>),
    /// A decryption error occurred (terminal state).
    DecErr,
}

/// The states for writing Noise protocol frames.
#[derive(Debug)]
enum WriteState {
    /// Ready to write another frame.
    Ready,
    /// Writing the frame length.
    WriteLen { len: usize, buf: [u8; 2], off: usize },
    /// Writing the frame data.
    WriteData { len: usize, off: usize },
    /// EOF has been reached unexpectedly (terminal state).
    Eof,
    /// An encryption error occurred (terminal state).
    EncErr,
}

// impl WriteState {
//     fn is_ready(&self) -> bool {
//         if let WriteState::Ready = self {
//             return true;
//         }
//         false
//     }
// }

impl<T, S> NoiseFramed<T, S>
where
    T: WriteEx + ReadEx + Unpin + Send,
    S: SessionState + Unpin,
{
    /// Read data
    pub(crate) async fn next(&mut self) -> Option<io::Result<Bytes>> {
        loop {
            match self.read_state {
                ReadState::Ready => self.read_state = ReadState::ReadLen { buf: [0, 0], off: 0 },
                ReadState::ReadLen { mut buf, mut off } => {
                    let n = match read_frame_len(&mut self.io, &mut buf, &mut off).await {
                        Ok(Some(n)) => n,
                        Ok(None) => {
                            trace!("read: eof");
                            self.read_state = ReadState::Eof(Ok(()));
                            return None;
                        }
                        Err(e) => {
                            return Some(Err(e));
                        }
                    };
                    trace!("read: frame len = {}", n);
                    if n == 0 {
                        trace!("read: empty frame");
                        self.read_state = ReadState::Ready;
                        continue;
                    }
                    self.read_buffer.resize(usize::from(n), 0u8);
                    self.read_state = ReadState::ReadData {
                        len: usize::from(n),
                        off: 0,
                    }
                }
                ReadState::ReadData { len, ref mut off } => {
                    let n = {
                        match self.io.read2(&mut self.read_buffer[*off..len]).await {
                            Ok(n) => n,
                            Err(e) => return Some(Err(e)),
                        }
                    };
                    trace!("read: {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("read: eof");
                        self.read_state = ReadState::Eof(Err(()));
                        return Some(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    *off += n;
                    if len == *off {
                        trace!("read: decrypting {} bytes", len);
                        self.decrypt_buffer.resize(len, 0);
                        if let Ok(n) = self.session.read_message(&self.read_buffer, &mut self.decrypt_buffer) {
                            self.decrypt_buffer.truncate(n);
                            trace!("read: payload len = {} bytes", n);
                            self.read_state = ReadState::Ready;
                            // Return an immutable view into the current buffer.
                            // If the view is dropped before the next frame is
                            // read, the `BytesMut` will reuse the same buffer
                            // for the next frame.
                            let view = self.decrypt_buffer.split().freeze();
                            return Some(Ok(view));
                        } else {
                            debug!("read: decryption error");
                            self.read_state = ReadState::DecErr;
                            return Some(Err(io::ErrorKind::InvalidData.into()));
                        }
                    }
                }
                ReadState::Eof(Ok(())) => {
                    trace!("read: eof");
                    // return None;
                }
                ReadState::Eof(Err(())) => {
                    trace!("read: eof (unexpected)");
                    return Some(Err(io::ErrorKind::UnexpectedEof.into()));
                }
                ReadState::DecErr => {
                    return Some(Err(io::ErrorKind::InvalidData.into()));
                }
            }
        }
    }

    /// Ready to send data
    pub(crate) async fn ready2(&mut self) -> io::Result<()> {
        loop {
            trace!("write state {:?}", self.write_state);
            match self.write_state {
                WriteState::Ready => {
                    return Ok(());
                }
                WriteState::WriteLen { len, buf, mut off } => {
                    trace!("write: frame len ({}, {:?}, {}/2)", len, buf, off);
                    match write_frame_len(&mut self.io, &buf, &mut off).await {
                        Ok(true) => (),
                        Ok(false) => {
                            trace!("write: eof");
                            self.write_state = WriteState::Eof;
                            return Err(io::ErrorKind::WriteZero.into());
                        }
                        Err(e) => {
                            return Err(e);
                        } // Poll::Pending => {
                          //     self.write_state = WriteState::WriteLen { len, buf, off };
                          //     return Poll::Pending;
                          // }
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = {
                        let f = self.io.write2(&self.write_buffer[*off..len]).await;
                        match f {
                            Ok(n) => n,
                            Err(e) => return Err(e),
                        }
                    };
                    if n == 0 {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into());
                    }
                    *off += n;
                    trace!("write: {}/{} bytes written", *off, len);
                    if len == *off {
                        trace!("write: finished with {} bytes", len);
                        self.write_state = WriteState::Ready;
                    }
                }
                WriteState::Eof => {
                    trace!("write: eof");
                    return Err(io::ErrorKind::WriteZero.into());
                }
                WriteState::EncErr => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
    }

    /// Use noise protocol to cipher data
    pub(crate) async fn send2(&mut self, frame: &[u8]) -> io::Result<()> {
        self.write_buffer.resize(frame.len() + EXTRA_ENCRYPT_SPACE, 0u8);
        match self.session.write_message(frame, &mut self.write_buffer[..]) {
            Ok(n) => {
                trace!("write: cipher text len = {} bytes", n);
                self.write_buffer.truncate(n);
                self.write_state = WriteState::WriteLen {
                    len: n,
                    buf: u16::to_be_bytes(n as u16),
                    off: 0,
                };
                Ok(())
            }
            Err(e) => {
                log::error!("encryption error: {:?}", e);
                self.write_state = WriteState::EncErr;
                Err(io::ErrorKind::InvalidData.into())
            }
        }
    }

    pub(crate) async fn flush2(&mut self) -> io::Result<()> {
        self.ready2().await?;
        self.io.flush2().await
    }

    pub(crate) async fn close2(&mut self) -> io::Result<()> {
        self.ready2().await?;
        self.io.close2().await
    }
}

/// A stateful context in which Noise protocol messages can be read and written.
pub trait SessionState {
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error>;
    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error>;
}

impl SessionState for snow::HandshakeState {
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.read_message(msg, buf)
    }

    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.write_message(msg, buf)
    }
}

impl SessionState for snow::TransportState {
    fn read_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.read_message(msg, buf)
    }

    fn write_message(&mut self, msg: &[u8], buf: &mut [u8]) -> Result<usize, snow::Error> {
        self.write_message(msg, buf)
    }
}

/// Read 2 bytes as frame length from the given source into the given buffer.
///
/// Panics if `off >= 2`.
///
/// When [`Poll::Pending`] is returned, the given buffer and offset
/// may have been updated (i.e. a byte may have been read) and must be preserved
/// for the next invocation.
///
/// Returns `None` if EOF has been encountered.
async fn read_frame_len<R: ReadEx + Unpin + Send>(io: &mut R, buf: &mut [u8; 2], off: &mut usize) -> io::Result<Option<u16>> {
    // match ready!(Pin::new(&mut io).poll_read(cx, &mut buf[*off ..])) {
    match io.read_exact2(&mut buf[*off..]).await {
        Ok(()) => Ok(Some(u16::from_be_bytes(*buf))),
        Err(e) => Err(e),
    }
    // match io.read2(&mut buf[*off..]).await {
    //     Ok(n) => {
    //         if n == 0 {
    //             return Ok(None);
    //         }
    //         *off += n;
    //         if *off == 2 {
    //             return Ok(Some(u16::from_be_bytes(*buf)));
    //         }
    //     }
    //     Err(e) => {
    //         return Err(e);
    //     }
    // }
}

/// Write 2 bytes as frame length from the given buffer into the given sink.
///
/// Panics if `off >= 2`.
///
/// When [`Poll::Pending`] is returned, the given offset
/// may have been updated (i.e. a byte may have been written) and must
/// be preserved for the next invocation.
///
/// Returns `false` if EOF has been encountered.
async fn write_frame_len<W: WriteEx + Unpin>(io: &mut W, buf: &[u8; 2], off: &mut usize) -> io::Result<bool> {
    loop {
        match io.write2(&buf[*off..]).await {
            Ok(n) => {
                if n == 0 {
                    return Ok(false);
                }
                *off += n;
                if *off == 2 {
                    return Ok(true);
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}
