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

/// Hmac struct on this module comes from `rust-libp2p`, but use high version of hamc

/// Encryption and decryption stream
pub mod secure_stream;

use crate::Digest;

/// Hash-based Message Authentication Code (HMAC).
#[derive(Debug, Clone)]
pub struct Hmac(ring::hmac::Key);

impl Hmac {
    /// Returns the size of the hash in bytes.
    #[inline]
    pub fn num_bytes(&self) -> usize {
        self.0.algorithm().digest_algorithm().output_len
    }

    /// Builds a `Hmac` from an algorithm and key.
    pub fn from_key(algorithm: Digest, key: &[u8]) -> Self {
        match algorithm {
            Digest::Sha256 => Hmac(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key)),
            Digest::Sha512 => Hmac(ring::hmac::Key::new(ring::hmac::HMAC_SHA512, key)),
        }
    }

    /// Signs the data.
    pub fn sign(&mut self, crypted_data: &[u8]) -> ring::hmac::Tag {
        ring::hmac::sign(&self.0, crypted_data)
    }

    /// Verifies that the data matches the expected hash.
    pub fn verify(&mut self, crypted_data: &[u8], expected_hash: &[u8]) -> bool {
        ring::hmac::verify(&self.0, crypted_data, expected_hash).is_ok()
    }

    /// Return a multi-step hmac context
    pub fn context(&self) -> ring::hmac::Context {
        ring::hmac::Context::with_key(&self.0)
    }
}
