// Copyright (c) 2019 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use std::{fmt, io};

use super::{
    header::{self, HeaderDecodeError},
    Frame,
};
use crate::connection::Id;
use crate::frame::header::HEADER_SIZE;

use libp2prs_traits::{ReadEx, WriteEx};

/// A [`Stream`] and writer of [`Frame`] values.
// #[derive(Debug)]
pub(crate) struct Io<T> {
    id: Id,
    io: T,
    max_body_len: usize,
    // pub(crate) stream: Pin<Box<dyn FusedStream<Item = Result<Frame<()>, FrameDecodeError>> + Send>>,
}

impl<T> fmt::Debug for Io<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Io[id: {:?}]", self.id)
    }
}

impl<T: Unpin + Send> Io<T> {
    pub(crate) fn new(id: Id, io: T, max_frame_body_len: usize) -> Self {
        Io {
            id,
            io,
            max_body_len: max_frame_body_len,
        }
    }
}

impl<T: ReadEx + Unpin + Send> Io<T> {
    pub(crate) async fn recv_frame(&mut self) -> Result<Frame<()>, FrameDecodeError> {
        let mut header_data = [0; HEADER_SIZE];

        self.io.read_exact2(&mut header_data).await?;
        let header = header::decode(&header_data)?;

        log::trace!("{}: read stream header: {}", self.id, header);

        if header.tag() != header::Tag::Data {
            return Ok(Frame::new(header));
        }

        let body_len = header.len().val() as usize;
        if body_len > self.max_body_len {
            return Err(FrameDecodeError::FrameTooLarge(body_len));
        }

        let mut body = vec![0; body_len];
        self.io.read_exact2(&mut body).await?;

        Ok(Frame { header, body })
    }
}

impl<T: WriteEx + Unpin + Send> Io<T> {
    pub(crate) async fn send_frame<A>(&mut self, frame: &Frame<A>) -> io::Result<()> {
        log::trace!("{}: write stream, header: {}", self.id, frame.header);

        let header = header::encode(&frame.header);

        // write flush can reduce the probability of secuio read failed Because split bug
        // Also reduce the number of secuio crypto
        // especially for test case of secuio + muxer
        use bytes::{BufMut, BytesMut};
        let mut buf = BytesMut::with_capacity(frame.body.len() + 12);
        buf.put(header.to_vec().as_ref());
        buf.put(&frame.body[..]);
        self.io.write_all2(&buf).await
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.io.flush2().await
    }
    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.io.close2().await
    }
}

#[allow(dead_code)]
struct FrameReader<T> {
    id: Id,
    io: T,
    max_body_len: usize,
}

#[allow(dead_code)]
impl<T: ReadEx + WriteEx + Unpin + Send> FrameReader<T> {
    pub(crate) fn new(id: Id, io: T, max_frame_body_len: usize) -> Self {
        FrameReader {
            id,
            io,
            max_body_len: max_frame_body_len,
        }
    }

    pub(crate) async fn recv_frame(&mut self) -> Result<Frame<()>, FrameDecodeError> {
        let mut header_data = [0; HEADER_SIZE];

        self.io.read_exact2(&mut header_data).await?;
        let header = header::decode(&header_data)?;

        log::trace!("{}: read stream header: {}", self.id, header);

        if header.tag() != header::Tag::Data {
            return Ok(Frame::new(header));
        }

        let body_len = header.len().val() as usize;
        if body_len > self.max_body_len {
            return Err(FrameDecodeError::FrameTooLarge(body_len));
        }

        let mut body = vec![0; body_len];
        self.io.read_exact2(&mut body).await?;

        Ok(Frame { header, body })
    }
}

/// Possible errors while decoding a message frame.
#[non_exhaustive]
#[derive(Debug)]
pub enum FrameDecodeError {
    /// An I/O error.
    Io(io::Error),
    /// Decoding the frame header failed.
    Header(HeaderDecodeError),
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

impl From<HeaderDecodeError> for FrameDecodeError {
    fn from(e: HeaderDecodeError) -> Self {
        FrameDecodeError::Header(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen, QuickCheck};
    use rand::RngCore;

    impl Arbitrary for Frame<()> {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let mut header: header::Header<()> = Arbitrary::arbitrary(g);
            let body = if header.tag() == header::Tag::Data {
                header.set_len(header.len().val() % 4096);
                let mut b = vec![0; header.len().val() as usize];
                rand::thread_rng().fill_bytes(&mut b);
                b
            } else {
                Vec::new()
            };
            Frame { header, body }
        }
    }

    #[test]
    fn encode_decode_identity() {
        fn property(f: Frame<()>) -> bool {
            async_std::task::block_on(async move {
                let id = crate::connection::Id::random(crate::connection::Mode::Server);
                let mut io = Io::new(id, futures::io::Cursor::new(Vec::new()), f.body.len());
                if io.send_frame(&f).await.is_err() {
                    return false;
                }
                if io.flush().await.is_err() {
                    return false;
                }
                io.io.set_position(0);
                if let Ok(x) = io.recv_frame().await {
                    x == f
                } else {
                    false
                }
            })
        }

        QuickCheck::new().tests(10_000).quickcheck(property as fn(Frame<()>) -> bool)
    }
}
