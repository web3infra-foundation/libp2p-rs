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

use crate::error::NoiseError::{Io, Noise};
use crate::io::framed::{read_frame_len, write_frame_len, ReadState, SessionState, WriteState};
use crate::NoiseError;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::io::Error;
use futures::lock::Mutex;
use libp2prs_traits::{ReadEx, SplitEx, WriteEx};
use log::{debug, trace};
use std::cmp::min;
use std::io;
use std::sync::Arc;

/// Max. size of a noise message.
const MAX_NOISE_MSG_LEN: usize = 65535;
/// Space given to the encryption buffer to hold key material.
const EXTRA_ENCRYPT_SPACE: usize = 1024;
/// Max. length for Noise protocol message payloads.
pub const MAX_FRAME_LEN: usize = MAX_NOISE_MSG_LEN - EXTRA_ENCRYPT_SPACE;

static_assertions::const_assert! {
    MAX_FRAME_LEN + EXTRA_ENCRYPT_SPACE <= MAX_NOISE_MSG_LEN
}

/// A `NoiseSecureStream` is an encrypt stream that contains
/// `NoiseSecureStreamReader` and `NoiseSecureStreamWriter`.
///
/// `R` & `W` is the type of the underlying I/O resource.
/// In this way, we use `snow::TransportState`.
pub struct NoiseSecureStream<R, W> {
    reader: NoiseSecureStreamReader<R>,
    writer: NoiseSecureStreamWriter<W>,
}

/// SecureStreamReader, `R` means the underlying I/O that supports
/// `ReadEx` + `Unpin` + `static`
pub struct NoiseSecureStreamReader<R> {
    io: R,
    session: Arc<Mutex<snow::TransportState>>,
    recv_offset: usize,
    read_state: ReadState,
    read_buffer: Vec<u8>,
    decrypt_buffer: BytesMut,
}

/// SecureStreamWriter, `W` means the underlying I/O that supports
/// `WriteEx` + `Unpin` + `static`
pub struct NoiseSecureStreamWriter<W> {
    io: W,
    session: Arc<Mutex<snow::TransportState>>,
    write_state: WriteState,
    send_offset: usize,
    plain_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

impl<R> NoiseSecureStreamReader<R>
where
    R: ReadEx + Unpin + 'static,
{
    /// new stream
    pub(crate) fn new(reader: R, state: Arc<Mutex<snow::TransportState>>) -> Self {
        NoiseSecureStreamReader {
            io: reader,
            session: state,
            recv_offset: 0,
            read_state: ReadState::Ready,
            read_buffer: BytesMut::new().to_vec(),
            decrypt_buffer: BytesMut::new(),
        }
    }

    /// Read data
    pub(crate) async fn next(&mut self) -> Option<Result<Bytes, NoiseError>> {
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
                            return Some(Err(NoiseError::Io(e)));
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
                    };
                }
                ReadState::ReadData { len, ref mut off } => {
                    let n = {
                        match self.io.read2(&mut self.read_buffer[*off..len]).await {
                            Ok(n) => n,
                            Err(e) => return Some(Err(NoiseError::Io(e))),
                        }
                    };
                    trace!("read: {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("read: eof");
                        self.read_state = ReadState::Eof(Err(()));
                        return Some(Err(Io(io::ErrorKind::UnexpectedEof.into())));
                    }
                    *off += n;
                    if len == *off {
                        trace!("read: decrypting {} bytes", len);
                        self.decrypt_buffer.resize(len, 0);
                        match self.session.lock().await.read_message(&self.read_buffer, &mut self.decrypt_buffer) {
                            Ok(n) => {
                                self.decrypt_buffer.truncate(n);
                                trace!("read: payload len = {} bytes", n);
                                self.read_state = ReadState::Ready;
                                // Return an immutable view into the current buffer.
                                // If the view is dropped before the next frame is
                                // read, the `Bytes` will reuse the same buffer
                                // for the next frame.
                                let view = self.decrypt_buffer.split().freeze();
                                return Some(Ok(view));
                            }
                            Err(e) => {
                                debug!("read: decryption error");
                                self.read_state = ReadState::DecErr;
                                return Some(Err(Noise(e)));
                            }
                        }
                    }
                }
                ReadState::Eof(Ok(())) => {
                    trace!("read: eof");
                }
                ReadState::Eof(Err(())) => {
                    trace!("read: eof (unexpected)");
                    return Some(Err(Io(io::ErrorKind::UnexpectedEof.into())));
                }
                ReadState::DecErr => {
                    return Some(Err(Io(io::ErrorKind::InvalidData.into())));
                }
            }
        }
    }
}

#[async_trait]
impl<R> ReadEx for NoiseSecureStreamReader<R>
where
    R: ReadEx + Unpin + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        loop {
            let len = self.read_buffer.len();
            let off = self.recv_offset;
            if len > 0 {
                let n = min(len - off, buf.len());
                buf[..n].copy_from_slice(&self.read_buffer[off..off + n]);
                trace!("read: copied {}/{} bytes", off + n, len);
                self.recv_offset += n;
                if len == self.recv_offset {
                    trace!("read: frame consumed");
                    // Drop the existing view so `StreamReader` can reuse
                    // the buffer when polling for the next frame below.
                    self.read_buffer = Bytes::new().to_vec();
                }
                return Ok(n);
            }

            match self.next().await {
                Some(Ok(frame)) => {
                    self.read_buffer = frame.to_vec();
                    self.recv_offset = 0;
                }
                None => return Ok(0),
                Some(Err(e)) => return Err(e.into()),
            }
        }
    }
}

impl<W> NoiseSecureStreamWriter<W>
where
    W: WriteEx + Unpin + 'static,
{
    /// new writer
    pub(crate) fn new(writer: W, state: Arc<Mutex<snow::TransportState>>) -> Self {
        NoiseSecureStreamWriter {
            io: writer,
            session: state,
            send_offset: 0,
            write_state: WriteState::Ready,
            write_buffer: Vec::new(),
            plain_buffer: Vec::new(),
        }
    }

    /// Ready and send data
    pub(crate) async fn ready_and_send(&mut self) -> Result<(), NoiseError> {
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
                            return Err(Io(io::ErrorKind::WriteZero.into()));
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = {
                        let f = self.io.write2(&self.write_buffer[*off..len]).await;
                        match f {
                            Ok(n) => n,
                            Err(e) => return Err(e.into()),
                        }
                    };
                    if n == 0 {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(Io(io::ErrorKind::WriteZero.into()));
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
                    return Err(Io(io::ErrorKind::WriteZero.into()));
                }
                WriteState::EncErr => return Err(Io(io::ErrorKind::InvalidData.into())),
            }
        }
    }

    /// Use noise protocol to cipher data
    pub(crate) async fn encrypt_data(&mut self, frame: &[u8]) -> Result<(), NoiseError> {
        self.write_buffer.resize(frame.len() + EXTRA_ENCRYPT_SPACE, 0u8);
        match self.session.lock().await.write_message(frame, &mut self.write_buffer[..]) {
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
                Err(NoiseError::Noise(e))
            }
        }
    }
}

#[async_trait]
impl<W> WriteEx for NoiseSecureStreamWriter<W>
where
    W: WriteEx + Unpin + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        // The MAX_FRAME_LEN is the maximum buffer size before a buffer must be sent.
        if self.send_offset == MAX_FRAME_LEN {
            trace!("write: sending {} bytes", MAX_FRAME_LEN);

            match self.encrypt_data(buf).await {
                Ok(()) => {}
                Err(e) => return Err(e.into()),
            }
            self.plain_buffer.clear();
            self.send_offset = 0;
        }

        let off = self.send_offset;
        let n = min(MAX_FRAME_LEN, off.saturating_add(buf.len()));
        self.plain_buffer.resize(n, 0u8);
        let n = min(MAX_FRAME_LEN - off, buf.len());
        self.plain_buffer[off..off + n].copy_from_slice(&buf[..n]);
        self.send_offset += n;
        trace!("write: buffered {} bytes", self.send_offset);

        let buf = self.plain_buffer.clone();

        match self.encrypt_data(&buf).await {
            Ok(()) => match self.flush2().await {
                Ok(()) => Ok(n),
                Err(e) => Err(e),
            },
            Err(e) => Err(e.into()),
        }
    }

    async fn flush2(&mut self) -> io::Result<()> {
        // Check if there is still one more buffer to send.
        if self.send_offset > 0 {
            match self.ready_and_send().await {
                Ok(()) => {}
                Err(e) => return Err(e.into()),
            }
            trace!("flush: sending {} bytes", self.send_offset);
            self.send_offset = 0;
        }

        Ok(())
    }

    async fn close2(&mut self) -> io::Result<()> {
        match self.ready_and_send().await {
            Ok(()) => self.io.close2().await,
            Err(e) => Err(e.into()),
        }
    }
}

impl<R, W> NoiseSecureStream<R, W>
where
    R: ReadEx + Unpin + 'static,
    W: WriteEx + Unpin + 'static,
{
    /// Creates a new `NoiseSecureStream` after Noise protocol handshake.
    pub fn new(reader: R, writer: W, state: snow::TransportState) -> Self {
        let session = Arc::new(Mutex::new(state));

        NoiseSecureStream {
            reader: NoiseSecureStreamReader::new(reader, session.clone()),
            writer: NoiseSecureStreamWriter::new(writer, session),
        }
    }
}

#[async_trait]
impl<R, W> ReadEx for NoiseSecureStream<R, W>
where
    R: ReadEx + Unpin + 'static,
    W: WriteEx + Unpin + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.reader.read2(buf).await
    }
}

#[async_trait]
impl<R, W> WriteEx for NoiseSecureStream<R, W>
where
    R: ReadEx + Unpin + 'static,
    W: WriteEx + Unpin + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write2(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.writer.flush2().await
    }
    async fn close2(&mut self) -> io::Result<()> {
        self.writer.close2().await
    }
}

impl<R, W> SplitEx for NoiseSecureStream<R, W>
where
    R: ReadEx + Unpin + 'static,
    W: WriteEx + Unpin + 'static,
{
    type Reader = NoiseSecureStreamReader<R>;
    type Writer = NoiseSecureStreamWriter<W>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (self.reader, self.writer)
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
