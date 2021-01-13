// Copyright 2019 Parity Technologies (UK) Ltd.
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

/// I borrowed the error type of `rust-libp2p`, deleted some error types, and added an error type.
use std::{error, fmt, io};

/// Error at the Plaintext layer communication.
#[derive(Debug)]
pub enum PlaintextError {
    /// I/O error.
    IoError(io::Error),

    /// Failed to generate the secret shared key from the ephemeral key.
    SecretGenerationFailed,

    /// Public key is empty
    EmptyPublicKey,

    /// Connect yourself
    ConnectSelf,

    /// Failed to parse one of the handshake bincode messages.
    HandshakeParsingFailure,

    /// ID and Public_key is not match
    MismatchedIDANDPubKey,

    /// Invalid message message found during handshake
    InvalidMessage,

    /// We received an invalid proposition from remote.
    InvalidProposition(&'static str),
}

impl PartialEq for PlaintextError {
    fn eq(&self, other: &PlaintextError) -> bool {
        use self::PlaintextError::*;
        match (self, other) {
            (InvalidProposition(i), InvalidProposition(j)) => i == j,
            (SecretGenerationFailed, SecretGenerationFailed)
            | (ConnectSelf, ConnectSelf)
            | (EmptyPublicKey, EmptyPublicKey)
            | (HandshakeParsingFailure, HandshakeParsingFailure)
            | (MismatchedIDANDPubKey, MismatchedIDANDPubKey)
            | (InvalidMessage, InvalidMessage) => true,
            _ => false,
        }
    }
}

impl From<io::Error> for PlaintextError {
    #[inline]
    fn from(err: io::Error) -> PlaintextError {
        PlaintextError::IoError(err)
    }
}

impl Into<io::Error> for PlaintextError {
    fn into(self) -> io::Error {
        match self {
            PlaintextError::IoError(e) => e,
            e => io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()),
        }
    }
}

impl error::Error for PlaintextError {}

impl fmt::Display for PlaintextError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PlaintextError::IoError(e) => fmt::Display::fmt(&e, f),
            PlaintextError::SecretGenerationFailed => write!(f, "Secret Generation Failed"),
            PlaintextError::MismatchedIDANDPubKey => write!(f, "ID & Public_key is not match"),
            PlaintextError::ConnectSelf => write!(f, "Connect Self"),
            PlaintextError::HandshakeParsingFailure => write!(f, "Handshake Parsing Failure"),
            PlaintextError::InvalidMessage => write!(f, "Invalid Message"),
            PlaintextError::InvalidProposition(e) => write!(f, "Invalid Proposition: {}", e),
            PlaintextError::EmptyPublicKey => write!(f, "Public key is empty"),
        }
    }
}
