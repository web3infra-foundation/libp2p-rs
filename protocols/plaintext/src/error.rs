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
