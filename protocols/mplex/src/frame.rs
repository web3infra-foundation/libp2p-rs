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

    pub fn body(&self) -> &[u8] {
        &self.body
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
