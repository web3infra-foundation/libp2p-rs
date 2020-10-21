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

use futures::future::Either;
use std::fmt;

/// The message frame header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header<T> {
    version: Version,
    tag: Tag,
    flags: Flags,
    stream_id: StreamId,
    length: Len,
    _marker: std::marker::PhantomData<T>,
}

impl<T> fmt::Display for Header<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Header {:?} {} (len {}) (flags {:?}))",
            self.tag,
            self.stream_id,
            self.length.val(),
            self.flags.val()
        )
    }
}

impl<T> Header<T> {
    pub fn tag(&self) -> Tag {
        self.tag
    }

    pub fn flags(&self) -> Flags {
        self.flags
    }

    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    pub fn len(&self) -> Len {
        self.length
    }

    pub fn set_len(&mut self, len: u32) {
        self.length = Len(len)
    }

    /// Arbitrary type cast, use with caution.
    fn cast<U>(self) -> Header<U> {
        Header {
            version: self.version,
            tag: self.tag,
            flags: self.flags,
            stream_id: self.stream_id,
            length: self.length,
            _marker: std::marker::PhantomData,
        }
    }
}

impl Header<()> {
    pub(crate) fn into_data(self) -> Header<Data> {
        debug_assert_eq!(self.tag, Tag::Data);
        self.cast()
    }

    pub(crate) fn into_window_update(self) -> Header<WindowUpdate> {
        debug_assert_eq!(self.tag, Tag::WindowUpdate);
        self.cast()
    }

    pub(crate) fn into_ping(self) -> Header<Ping> {
        debug_assert_eq!(self.tag, Tag::Ping);
        self.cast()
    }
}

impl<T: HasSyn> Header<T> {
    /// Set the [`SYN`] flag.
    pub fn syn(&mut self) {
        self.flags.0 |= SYN.0
    }
}

impl<T: HasAck> Header<T> {
    /// Set the [`ACK`] flag.
    pub fn ack(&mut self) {
        self.flags.0 |= ACK.0
    }
}

impl<T: HasFin> Header<T> {
    /// Set the [`FIN`] flag.
    pub fn fin(&mut self) {
        self.flags.0 |= FIN.0
    }
}

impl<T: HasRst> Header<T> {
    /// Set the [`RST`] flag.
    pub fn rst(&mut self) {
        self.flags.0 |= RST.0
    }
}

impl Header<Data> {
    /// Create a new data frame header.
    pub fn data(id: StreamId, len: u32) -> Self {
        Header {
            version: Version(0),
            tag: Tag::Data,
            flags: Flags(0),
            stream_id: id,
            length: Len(len),
            _marker: std::marker::PhantomData,
        }
    }
}

impl Header<WindowUpdate> {
    /// Create a new window update frame header.
    pub fn window_update(id: StreamId, credit: u32) -> Self {
        Header {
            version: Version(0),
            tag: Tag::WindowUpdate,
            flags: Flags(0),
            stream_id: id,
            length: Len(credit),
            _marker: std::marker::PhantomData,
        }
    }

    /// The credit this window update grants to the remote.
    pub fn credit(&self) -> usize {
        self.length.0 as usize
    }
}

impl Header<Ping> {
    /// Create a new ping frame header.
    pub fn ping(nonce: u32) -> Self {
        Header {
            version: Version(0),
            tag: Tag::Ping,
            flags: Flags(0),
            stream_id: StreamId(0),
            length: Len(nonce),
            _marker: std::marker::PhantomData,
        }
    }

    /// The nonce of this ping.
    pub fn nonce(&self) -> u32 {
        self.length.0
    }
}

impl Header<GoAway> {
    /// Terminate the session without indicating an error to the remote.
    pub fn term() -> Self {
        Self::go_away(0)
    }

    /// Terminate the session indicating a protocol error to the remote.
    pub fn protocol_error() -> Self {
        Self::go_away(1)
    }

    /// Terminate the session indicating an internal error to the remote.
    pub fn internal_error() -> Self {
        Self::go_away(2)
    }

    fn go_away(code: u32) -> Self {
        Header {
            version: Version(0),
            tag: Tag::GoAway,
            flags: Flags(0),
            stream_id: StreamId(0),
            length: Len(code),
            _marker: std::marker::PhantomData,
        }
    }
}

/// Data message type.
#[derive(Clone, Debug)]
pub enum Data {}

/// Window update message type.
#[derive(Clone, Debug)]
pub enum WindowUpdate {}

/// Ping message type.
#[derive(Clone, Debug)]
pub enum Ping {}

/// Go Away message type.
#[derive(Clone, Debug)]
pub enum GoAway {}

/// Types which have a `syn` method.
pub trait HasSyn: private::Sealed {}
impl HasSyn for Data {}
impl HasSyn for WindowUpdate {}
impl HasSyn for Ping {}
impl<A: HasSyn, B: HasSyn> HasSyn for Either<A, B> {}

/// Types which have an `ack` method.
pub trait HasAck: private::Sealed {}
impl HasAck for Data {}
impl HasAck for WindowUpdate {}
impl HasAck for Ping {}
impl<A: HasAck, B: HasAck> HasAck for Either<A, B> {}

/// Types which have a `fin` method.
pub trait HasFin: private::Sealed {}
impl HasFin for Data {}
impl HasFin for WindowUpdate {}

/// Types which have a `rst` method.
pub trait HasRst: private::Sealed {}
impl HasRst for Data {}
impl HasRst for WindowUpdate {}

mod private {
    pub trait Sealed {}

    impl Sealed for super::Data {}
    impl Sealed for super::WindowUpdate {}
    impl Sealed for super::Ping {}
    impl<A: Sealed, B: Sealed> Sealed for super::Either<A, B> {}
}

/// A tag is the runtime representation of a message type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    Data,
    WindowUpdate,
    Ping,
    GoAway,
}

/// The protocol version a message corresponds to.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Version(u8);

/// The message length.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Len(u32);

impl Len {
    pub fn val(self) -> u32 {
        self.0
    }
}

pub const CONNECTION_ID: StreamId = StreamId(0);

/// The ID of a stream.
///
/// The value 0 denotes no particular stream but the whole session.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StreamId(u32);

impl StreamId {
    pub(crate) fn new(val: u32) -> Self {
        StreamId(val)
    }

    pub fn is_server(self) -> bool {
        self.0 % 2 == 0
    }

    pub fn is_client(self) -> bool {
        !self.is_server()
    }

    pub fn is_session(self) -> bool {
        self == CONNECTION_ID
    }

    pub fn val(self) -> u32 {
        self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// impl std::hash::Hash for StreamId {
//     fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
//         hasher.write_u32(self.0)
//     }
// }

impl nohash_hasher::IsEnabled for StreamId {}

/// Possible flags set on a message.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Flags(u16);

impl Flags {
    pub fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn val(self) -> u16 {
        self.0
    }
}

const MAX_FLAG_VAL: u16 = 8;

/// Indicates the start of a new stream.
pub const SYN: Flags = Flags(1);

/// Acknowledges the start of a new stream.
pub const ACK: Flags = Flags(2);

/// Indicates the half-closing of a stream.
pub const FIN: Flags = Flags(4);

/// Indicates an immediate stream reset.
pub const RST: Flags = Flags(8);

/// The serialised header size in bytes.
pub const HEADER_SIZE: usize = 12;

/// Encode a [`Header`] value.
pub fn encode<T>(hdr: &Header<T>) -> [u8; HEADER_SIZE] {
    let mut buf = [0; HEADER_SIZE];
    buf[0] = hdr.version.0;
    buf[1] = hdr.tag as u8;
    buf[2..4].copy_from_slice(&hdr.flags.0.to_be_bytes());
    buf[4..8].copy_from_slice(&hdr.stream_id.0.to_be_bytes());
    buf[8..HEADER_SIZE].copy_from_slice(&hdr.length.0.to_be_bytes());
    buf
}

/// Decode a [`Header`] value.
pub fn decode(buf: &[u8; HEADER_SIZE]) -> Result<Header<()>, HeaderDecodeError> {
    if buf[0] != 0 {
        return Err(HeaderDecodeError::Version(buf[0]));
    }

    let hdr = Header {
        version: Version(buf[0]),
        tag: match buf[1] {
            0 => Tag::Data,
            1 => Tag::WindowUpdate,
            2 => Tag::Ping,
            3 => Tag::GoAway,
            t => return Err(HeaderDecodeError::Type(t)),
        },
        flags: Flags(u16::from_be_bytes([buf[2], buf[3]])),
        stream_id: StreamId(u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]])),
        length: Len(u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]])),
        _marker: std::marker::PhantomData,
    };

    if hdr.flags.0 > MAX_FLAG_VAL {
        return Err(HeaderDecodeError::Flags(hdr.flags.0));
    }

    Ok(hdr)
}

/// Possible errors while decoding a message frame header.
#[non_exhaustive]
#[derive(Debug)]
pub enum HeaderDecodeError {
    /// Unknown version.
    Version(u8),
    /// An unknown frame type.
    Type(u8),
    /// Unknown flags.
    Flags(u16),
}

impl std::fmt::Display for HeaderDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HeaderDecodeError::Version(v) => write!(f, "unknown version: {}", v),
            HeaderDecodeError::Type(t) => write!(f, "unknown frame type: {}", t),
            HeaderDecodeError::Flags(x) => write!(f, "unknown flags type: {}", x),
        }
    }
}

impl std::error::Error for HeaderDecodeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen, QuickCheck};
    use rand::{seq::SliceRandom, Rng};

    impl Arbitrary for Header<()> {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let tag = [Tag::Data, Tag::WindowUpdate, Tag::Ping, Tag::GoAway].choose(g).unwrap().clone();

            Header {
                version: Version(0),
                tag,
                flags: Flags(std::cmp::min(g.gen(), MAX_FLAG_VAL)),
                stream_id: StreamId(g.gen()),
                length: Len(g.gen()),
                _marker: std::marker::PhantomData,
            }
        }
    }

    #[test]
    fn encode_decode_identity() {
        fn property(hdr: Header<()>) -> bool {
            match decode(&encode(&hdr)) {
                Ok(x) => x == hdr,
                Err(e) => {
                    eprintln!("decode error: {}", e);
                    false
                }
            }
        }
        QuickCheck::new().tests(10_000).quickcheck(property as fn(Header<()>) -> bool)
    }
}
