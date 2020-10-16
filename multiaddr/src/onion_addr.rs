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

use serde::export::Formatter;
use std::borrow::Cow;
use std::fmt;
use std::fmt::Debug;

/// Represents an Onion v3 address
#[derive(Clone)]
pub struct Onion3Addr<'a>(Cow<'a, [u8; 35]>, u16);

impl<'a> Onion3Addr<'a> {
    /// Return the hash of the public key as bytes
    pub fn hash(&self) -> &[u8; 35] {
        self.0.as_ref()
    }

    /// Return the port
    pub fn port(&self) -> u16 {
        self.1
    }

    /// Consume this instance and create an owned version containing the same address
    pub fn acquire<'b>(self) -> Onion3Addr<'b> {
        Onion3Addr(Cow::Owned(self.0.into_owned()), self.1)
    }
}

impl PartialEq for Onion3Addr<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1 && self.0[..] == other.0[..]
    }
}

impl Eq for Onion3Addr<'_> {}

impl From<([u8; 35], u16)> for Onion3Addr<'_> {
    fn from(parts: ([u8; 35], u16)) -> Self {
        Self(Cow::Owned(parts.0), parts.1)
    }
}

impl<'a> From<(&'a [u8; 35], u16)> for Onion3Addr<'a> {
    fn from(parts: (&'a [u8; 35], u16)) -> Self {
        Self(Cow::Borrowed(parts.0), parts.1)
    }
}

impl Debug for Onion3Addr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_tuple("Onion3Addr")
            .field(&format!("{:02x?}", &self.0[..]))
            .field(&self.1)
            .finish()
    }
}
