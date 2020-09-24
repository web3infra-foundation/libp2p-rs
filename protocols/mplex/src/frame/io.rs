use std::io;

use crate::connection::Id;
use crate::frame::header;
use crate::frame::length_delimited::LengthDelimited;
use crate::frame::Frame;
use libp2p_traits::{Read2, Write2};

const MAX_MESSAGE_SIZE: u32 = 1 << 20;

pub struct IO<T> {
    id: Id,
    io: LengthDelimited<T>,
}

impl<T> IO<T>
where
    T: Unpin + Send,
{
    pub(crate) fn new(id: Id, io: T) -> Self {
        let io = LengthDelimited::new(io, MAX_MESSAGE_SIZE);
        IO { id, io }
    }
}

impl<T> IO<T>
where
    T: Read2 + Unpin + Send,
{
    pub(crate) async fn recv_frame(&mut self) -> Result<Frame, FrameDecodeError> {
        // get header
        let header_byte = self.io.read_uvarint().await?;
        let header = header::decode(header_byte)?;

        log::trace!("{}: read stream header: {}", self.id, header);

        // get length
        let len = self.io.read_uvarint().await?;
        if len > MAX_MESSAGE_SIZE {
            return Err(FrameDecodeError::FrameTooLarge(len as usize));
        }
        if len == 0 {
            return Ok(Frame {
                header,
                body: Vec::new(),
            });
        }

        // get body
        let mut body = vec![0; len as usize];
        self.io.read_body(&mut body).await?;
        Ok(Frame { header, body })
    }
}

impl<T> IO<T>
where
    T: Write2 + Unpin + Send,
{
    pub(crate) async fn send_frame(&mut self, frame: &Frame) -> io::Result<()> {
        log::trace!(
            "{}: write stream, header: {}, len {}",
            self.id,
            frame.header,
            frame.body.len()
        );

        let hdr = header::encode(&frame.header);

        self.io.write_header(hdr).await?;
        self.io.write_length(frame.body.len() as u32).await?;
        if !frame.body.is_empty() {
            self.io.write_body(&frame.body).await?;
        }
        self.io.flush().await
    }

    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.io.close().await
    }
}

/// Possible errors while decoding a message frame.
#[non_exhaustive]
#[derive(Debug)]
pub enum FrameDecodeError {
    /// An I/O error.
    Io(io::Error),
    /// Decoding the frame header failed.
    Header(header::HeaderDecodeError),
    /// A data frame body length is larger than the configured maximum.
    FrameTooLarge(usize),
}

impl std::fmt::Display for FrameDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FrameDecodeError::Io(e) => write!(f, "i/o error: {}", e),
            FrameDecodeError::Header(e) => write!(f, "decode error: {}", e),
            FrameDecodeError::FrameTooLarge(n) => write!(f, "frame body is too large ({})", n),
        }
    }
}

impl std::error::Error for FrameDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrameDecodeError::Io(e) => Some(e),
            FrameDecodeError::Header(e) => Some(e),
            FrameDecodeError::FrameTooLarge(_) => None,
        }
    }
}

impl From<std::io::Error> for FrameDecodeError {
    fn from(e: std::io::Error) -> Self {
        FrameDecodeError::Io(e)
    }
}

impl From<header::HeaderDecodeError> for FrameDecodeError {
    fn from(e: header::HeaderDecodeError) -> Self {
        FrameDecodeError::Header(e)
    }
}
