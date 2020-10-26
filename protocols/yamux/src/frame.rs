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

pub mod header;
pub(crate) mod io;

use header::{Data, GoAway, Header, Ping, StreamId, WindowUpdate};
use std::{convert::TryInto, num::TryFromIntError};

pub use io::FrameDecodeError;

/// A Yamux message frame consisting of header and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame<T> {
    header: Header<T>,
    body: Vec<u8>,
}

impl<T> Frame<T> {
    pub fn new(header: Header<T>) -> Self {
        Frame { header, body: Vec::new() }
    }

    pub fn header(&self) -> &Header<T> {
        &self.header
    }

    pub fn header_mut(&mut self) -> &mut Header<T> {
        &mut self.header
    }
}

impl Frame<()> {
    pub(crate) fn into_data(self) -> Frame<Data> {
        Frame {
            header: self.header.into_data(),
            body: self.body,
        }
    }

    pub(crate) fn into_window_update(self) -> Frame<WindowUpdate> {
        Frame {
            header: self.header.into_window_update(),
            body: self.body,
        }
    }

    pub(crate) fn into_ping(self) -> Frame<Ping> {
        Frame {
            header: self.header.into_ping(),
            body: self.body,
        }
    }
}

impl Frame<Data> {
    pub fn data(id: StreamId, b: Vec<u8>) -> Result<Self, TryFromIntError> {
        Ok(Frame {
            header: Header::data(id, b.len().try_into()?),
            body: b,
        })
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn body_len(&self) -> u32 {
        // Safe cast since we construct `Frame::<Data>`s only with
        // `Vec<u8>` of length [0, u32::MAX] in `Frame::data` above.
        self.body().len() as u32
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    pub fn truncate(&mut self, n: usize) {
        self.header.set_len(n as u32);
        self.body.truncate(n);
    }
}

impl Frame<WindowUpdate> {
    pub fn window_update(id: StreamId, credit: u32) -> Self {
        Frame {
            header: Header::window_update(id, credit),
            body: Vec::new(),
        }
    }
}

impl Frame<GoAway> {
    pub fn term() -> Self {
        Frame {
            header: Header::term(),
            body: Vec::new(),
        }
    }

    pub fn protocol_error() -> Self {
        Frame {
            header: Header::protocol_error(),
            body: Vec::new(),
        }
    }

    pub fn internal_error() -> Self {
        Frame {
            header: Header::internal_error(),
            body: Vec::new(),
        }
    }
}
