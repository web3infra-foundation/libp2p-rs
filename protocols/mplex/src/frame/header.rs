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

use std::fmt;

/// The ID of a stream. It is identified via id and initiator.
/// initiator is true when open stream, Otherwise, false when accept stream
#[derive(Copy, Clone, Debug, Eq, PartialOrd, Ord)]
pub struct StreamID {
    id: u32,
    initiator: bool,
}

impl StreamID {
    pub(crate) fn new(id: u32, initiator: bool) -> Self {
        StreamID { id, initiator }
    }

    /// identify by u32, only used by swarm
    pub fn val(self) -> u32 {
        if self.initiator {
            return self.id + 10000000;
        }
        self.id
    }

    /// used by mplex protocol
    pub fn id(self) -> u32 {
        self.id
    }
}

impl fmt::Display for StreamID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl PartialEq for StreamID {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.initiator == other.initiator
    }
}

// HashMap insert() required key impl Hash trait
impl std::hash::Hash for StreamID {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        hasher.write_u32(self.id);
    }
}
impl nohash_hasher::IsEnabled for StreamID {}

/// A tag is the runtime representation of a message type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    NewStream = 0,
    Message = 2,
    Close = 4,
    Reset = 6,
}

/// The message frame header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    stream_id: StreamID,
    tag: Tag,
}

impl Header {
    pub fn new(stream_id: StreamID, tag: Tag) -> Self {
        Header { stream_id, tag }
    }

    pub fn tag(&self) -> Tag {
        self.tag
    }

    pub fn stream_id(&self) -> StreamID {
        self.stream_id
    }
}
impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(Header {:?} {})", self.tag, self.stream_id,)
    }
}

pub(crate) fn encode(hdr: &Header) -> u32 {
    let tag = hdr.tag as u32;
    let id = hdr.stream_id.id;
    if tag == 0 || hdr.stream_id.initiator {
        return (id << 3) + tag;
    }

    (id << 3) + tag - 1
}

/// Decode a [`Header`] value.
pub(crate) fn decode(header_byte: u32) -> Result<Header, HeaderDecodeError> {
    let tag = (header_byte & 7) as u8;
    let remote_initiator = tag & 1 == 0;
    let header = Header {
        stream_id: StreamID::new(header_byte >> 3, !remote_initiator),
        tag: match tag {
            0 => Tag::NewStream,
            1 | 2 => Tag::Message,
            3 | 4 => Tag::Close,
            5 | 6 => Tag::Reset,
            t => return Err(HeaderDecodeError::Tag(t)),
        },
    };

    Ok(header)
}

/// Possible errors while decoding a message frame header.
#[non_exhaustive]
#[derive(Debug)]
pub enum HeaderDecodeError {
    /// An unknown frame type.
    Tag(u8),
}

impl std::fmt::Display for HeaderDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HeaderDecodeError::Tag(t) => write!(f, "unknown frame type: {}", t),
        }
    }
}

impl std::error::Error for HeaderDecodeError {}
