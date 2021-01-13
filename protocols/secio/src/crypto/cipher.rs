// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::crypto::ctr_impl::CTR128LEN;
use ring::aead;

/// Possible encryption ciphers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CipherType {
    /// Aes128Ctr
    Aes128Ctr,
    /// Aes128Gcm
    Aes128Gcm,
    /// Aes256Gcm
    Aes256Gcm,
    /// ChaCha20Poly1305
    ChaCha20Poly1305,
}

impl CipherType {
    /// Returns the size of in bytes of the key expected by the cipher.
    pub fn key_size(self) -> usize {
        match self {
            CipherType::Aes128Ctr => CTR128LEN,
            CipherType::Aes128Gcm => aead::AES_128_GCM.key_len(),
            CipherType::Aes256Gcm => aead::AES_256_GCM.key_len(),
            CipherType::ChaCha20Poly1305 => aead::CHACHA20_POLY1305.key_len(),
        }
    }

    /// Returns the size of in bytes of the IV expected by the cipher.
    #[inline]
    pub fn iv_size(self) -> usize {
        match self {
            CipherType::Aes128Ctr => CTR128LEN,
            CipherType::Aes128Gcm => aead::AES_128_GCM.nonce_len(),
            CipherType::Aes256Gcm => aead::AES_256_GCM.nonce_len(),
            CipherType::ChaCha20Poly1305 => aead::CHACHA20_POLY1305.nonce_len(),
        }
    }

    /// Returns the size of in bytes of the tag expected by the cipher.
    #[inline]
    pub fn tag_size(self) -> usize {
        match self {
            CipherType::Aes128Ctr => 0,
            CipherType::Aes128Gcm => aead::AES_128_GCM.tag_len(),
            CipherType::Aes256Gcm => aead::AES_256_GCM.tag_len(),
            CipherType::ChaCha20Poly1305 => aead::CHACHA20_POLY1305.tag_len(),
        }
    }
}
