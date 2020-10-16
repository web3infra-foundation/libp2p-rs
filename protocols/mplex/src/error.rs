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

use crate::frame::FrameDecodeError;

/// The various error cases a connection may encounter.
#[non_exhaustive]
#[derive(Debug)]
pub enum ConnectionError {
    /// An underlying I/O error occured.
    Io(std::io::Error),
    /// Decoding a Yamux message frame failed.
    Decode(FrameDecodeError),
    /// The whole range of stream IDs has been used up.
    NoMoreStreamIds,
    /// An operation fails because the connection is closed.
    Closed,
    /// Too many streams are open, so no further ones can be opened at this time.
    TooManyStreams,
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ConnectionError::Io(e) => write!(f, "i/o error: {}", e),
            ConnectionError::Decode(e) => write!(f, "decode error: {}", e),
            ConnectionError::NoMoreStreamIds => f.write_str("number of stream ids has been exhausted"),
            ConnectionError::Closed => f.write_str("connection is closed"),
            ConnectionError::TooManyStreams => f.write_str("maximum number of streams reached"),
        }
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConnectionError::Io(e) => Some(e),
            ConnectionError::Decode(e) => Some(e),
            ConnectionError::NoMoreStreamIds | ConnectionError::Closed | ConnectionError::TooManyStreams => None,
        }
    }
}

impl From<std::io::Error> for ConnectionError {
    fn from(e: std::io::Error) -> Self {
        ConnectionError::Io(e)
    }
}

impl From<FrameDecodeError> for ConnectionError {
    fn from(e: FrameDecodeError) -> Self {
        ConnectionError::Decode(e)
    }
}

impl From<futures::channel::mpsc::SendError> for ConnectionError {
    fn from(_: futures::channel::mpsc::SendError) -> Self {
        ConnectionError::Closed
    }
}

impl From<futures::channel::oneshot::Canceled> for ConnectionError {
    fn from(_: futures::channel::oneshot::Canceled) -> Self {
        ConnectionError::Closed
    }
}
