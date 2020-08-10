//! Errors during identity key operations.

use std::error::Error;
use std::fmt;

/// An error during decoding of key material.
#[derive(Debug)]
pub struct DecodingError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>
}

impl DecodingError {
    pub(crate) fn new<S: ToString>(msg: S) -> Self {
        Self { msg: msg.to_string(), source: None }
    }

    pub(crate) fn source(self, source: impl Error + Send + Sync + 'static) -> Self {
        Self { source: Some(Box::new(source)), .. self }
    }
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Key decoding error: {}", self.msg)
    }
}

impl Error for DecodingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

/// An error during signing of a message.
#[derive(Debug)]
pub struct SigningError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>
}

/// An error during encoding of key material.
impl SigningError {
    pub(crate) fn new<S: ToString>(msg: S) -> Self {
        Self { msg: msg.to_string(), source: None }
    }

    pub(crate) fn source(self, source: impl Error + Send + Sync + 'static) -> Self {
        Self { source: Some(Box::new(source)), .. self }
    }
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Key signing error: {}", self.msg)
    }
}

impl Error for SigningError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

