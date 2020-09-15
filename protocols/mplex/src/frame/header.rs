use std::fmt;

/// The ID of a stream.
///
/// The value 0 denotes no particular stream but the whole session.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, Ord)]
pub struct StreamID {
    id: u32,
    initiator: bool,
}

impl StreamID {
    pub(crate) fn new(id: u32, initiator: bool) -> Self {
        StreamID { id, initiator }
    }

    pub fn val(self) -> u32 {
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
        hasher.write_u32(self.id)
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
    if tag == 0 || hdr.stream_id.initiator {
        return (hdr.stream_id.id << 3) + tag;
    }

    (hdr.stream_id.id << 3) + tag - 1
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
