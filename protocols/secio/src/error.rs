// Copyright 2017 Parity Technologies (UK) Ltd.
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

/// Error at the SECIO layer communication.
#[derive(Debug)]
pub enum SecioError {
    /// I/O error.
    IoError(io::Error),

    /// Ring Crypto error
    RingCryptoError,

    /// Failed to generate ephemeral key.
    EphemeralKeyGenerationFailed,

    /// Failed to generate the secret shared key from the ephemeral key.
    SecretGenerationFailed,

    /// There is no protocol supported by both the local and remote hosts.
    NoSupportIntersection,

    /// The final check of the handshake failed.
    NonceVerificationFailed,

    /// The received frame was of invalid length.
    FrameTooShort,

    /// The hashes of the message didn't match.
    HmacNotMatching,

    /// Connect yourself
    ConnectSelf,

    /// Failed to parse one of the handshake bincode messages.
    HandshakeParsingFailure,

    /// The signature of the exchange packet doesn't verify the remote public key.
    SignatureVerificationFailed,

    /// Invalid message message found during handshake
    InvalidMessage,

    /// We received an invalid proposition from remote.
    InvalidProposition(&'static str),
}

impl PartialEq for SecioError {
    fn eq(&self, other: &SecioError) -> bool {
        use self::SecioError::*;
        match (self, other) {
            (InvalidProposition(i), InvalidProposition(j)) => i == j,
            (EphemeralKeyGenerationFailed, EphemeralKeyGenerationFailed)
            | (SecretGenerationFailed, SecretGenerationFailed)
            | (NoSupportIntersection, NoSupportIntersection)
            | (NonceVerificationFailed, NonceVerificationFailed)
            | (FrameTooShort, FrameTooShort)
            | (HmacNotMatching, HmacNotMatching)
            | (ConnectSelf, ConnectSelf)
            | (HandshakeParsingFailure, HandshakeParsingFailure)
            | (SignatureVerificationFailed, SignatureVerificationFailed)
            | (InvalidMessage, InvalidMessage) => true,
            _ => false,
        }
    }
}

impl From<io::Error> for SecioError {
    #[inline]
    fn from(err: io::Error) -> SecioError {
        SecioError::IoError(err)
    }
}

impl Into<io::Error> for SecioError {
    fn into(self) -> io::Error {
        match self {
            SecioError::IoError(e) => e,
            e => io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()),
        }
    }
}

impl From<ring::error::Unspecified> for SecioError {
    fn from(_err: ring::error::Unspecified) -> SecioError {
        SecioError::RingCryptoError
    }
}

impl error::Error for SecioError {}

impl fmt::Display for SecioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SecioError::IoError(e) => fmt::Display::fmt(&e, f),
            SecioError::RingCryptoError => write!(f, "Ring Crypto Error"),
            SecioError::EphemeralKeyGenerationFailed => write!(f, "EphemeralKey Generation Failed"),
            SecioError::SecretGenerationFailed => write!(f, "Secret Generation Failed"),
            SecioError::NoSupportIntersection => write!(f, "No Support Intersection"),
            SecioError::NonceVerificationFailed => write!(f, "Nonce Verification Failed"),
            SecioError::FrameTooShort => write!(f, "Frame Too Short"),
            SecioError::HmacNotMatching => write!(f, "Hmac Not Matching"),
            SecioError::ConnectSelf => write!(f, "Connect Self"),
            SecioError::HandshakeParsingFailure => write!(f, "Handshake Parsing Failure"),
            SecioError::InvalidMessage => write!(f, "Invalid Message"),
            SecioError::SignatureVerificationFailed => write!(f, "Signature Verification Failed"),
            SecioError::InvalidProposition(e) => write!(f, "Invalid Proposition: {}", e),
        }
    }
}
