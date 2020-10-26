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

pub(crate) mod header;
pub(crate) mod io;
mod length_delimited;

pub(crate) use header::{Header, StreamID, Tag};
pub use io::FrameDecodeError;

/// A Yamux message frame consisting of header and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    header: Header,
    body: Vec<u8>,
}

impl Frame {
    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn stream_id(&self) -> StreamID {
        self.header.stream_id()
    }

    pub fn body(self) -> Vec<u8> {
        self.body
    }

    // new stream frame
    pub fn new_stream_frame(stream_id: StreamID, body: &[u8]) -> Self {
        Frame {
            header: Header::new(stream_id, Tag::NewStream),
            body: body.to_vec(),
        }
    }

    // message frame
    pub fn message_frame(stream_id: StreamID, body: &[u8]) -> Self {
        Frame {
            header: Header::new(stream_id, Tag::Message),
            body: body.to_vec(),
        }
    }

    // close frame
    pub fn close_frame(stream_id: StreamID) -> Self {
        Frame {
            header: Header::new(stream_id, Tag::Close),
            body: Vec::new(),
        }
    }

    // reset frame
    pub fn reset_frame(stream_id: StreamID) -> Self {
        Frame {
            header: Header::new(stream_id, Tag::Reset),
            body: Vec::new(),
        }
    }
}
