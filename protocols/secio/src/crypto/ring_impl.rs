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

use bytes::BytesMut;
use ring::{
    aead::{Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, AES_128_GCM, AES_256_GCM, CHACHA20_POLY1305},
    error::Unspecified,
};

use std::ptr;

use crate::{
    crypto::{cipher::CipherType, nonce_advance, CryptoMode, StreamCipher},
    error::SecioError,
};
use log::trace;

struct RingNonce(BytesMut);

impl NonceSequence for RingNonce {
    fn advance(&mut self) -> Result<Nonce, Unspecified> {
        nonce_advance(self.0.as_mut());
        Nonce::try_assume_unique_for_key(&self.0)
    }
}

enum RingAeadCryptoVariant {
    Seal(SealingKey<RingNonce>),
    Open(OpeningKey<RingNonce>),
}

pub(crate) struct RingAeadCipher {
    cipher: RingAeadCryptoVariant,
    cipher_type: CipherType,
}

impl RingAeadCipher {
    pub fn new(cipher_type: CipherType, key: &[u8], mode: CryptoMode) -> Self {
        trace!("New RingAeadCipher");
        let nonce_size = cipher_type.iv_size();
        let mut nonce = BytesMut::with_capacity(nonce_size);
        unsafe {
            nonce.set_len(nonce_size);
            ptr::write_bytes(nonce.as_mut_ptr(), 0, nonce_size);
        }

        let algorithm = match cipher_type {
            CipherType::Aes128Gcm => &AES_128_GCM,
            CipherType::Aes256Gcm => &AES_256_GCM,
            CipherType::ChaCha20Poly1305 => &CHACHA20_POLY1305,
            _ => panic!("Cipher type {:?} does not supported by RingAead yet", cipher_type),
        };

        let cipher = match mode {
            CryptoMode::Encrypt => {
                RingAeadCryptoVariant::Seal(SealingKey::new(UnboundKey::new(algorithm, key).unwrap(), RingNonce(nonce)))
            }
            CryptoMode::Decrypt => {
                RingAeadCryptoVariant::Open(OpeningKey::new(UnboundKey::new(algorithm, key).unwrap(), RingNonce(nonce)))
            }
        };
        RingAeadCipher { cipher, cipher_type }
    }

    /// Encrypt `input` to `output` with `tag`. `output.len()` should equals to `input.len() + tag.len()`.
    /// ```plain
    /// +----------------------------------------+-----------------------+
    /// | ENCRYPTED TEXT (length = input.len())  | TAG                   |
    /// +----------------------------------------+-----------------------+
    /// ```
    pub fn encrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        let mut output = Vec::with_capacity(input.len() + self.cipher_type.tag_size());
        unsafe {
            output.set_len(input.len());
        }
        output.copy_from_slice(input);
        if let RingAeadCryptoVariant::Seal(ref mut key) = self.cipher {
            key.seal_in_place_append_tag(Aad::empty(), &mut output)
                .map_err::<SecioError, _>(Into::into)?;
            Ok(output)
        } else {
            unreachable!("encrypt is called on a non-seal cipher")
        }
    }

    /// Decrypt `input` to `output` with `tag`. `output.len()` should equals to `input.len() - tag.len()`.
    /// ```plain
    /// +----------------------------------------+-----------------------+
    /// | ENCRYPTED TEXT (length = output.len()) | TAG                   |
    /// +----------------------------------------+-----------------------+
    /// ```
    pub fn decrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        let output_len = input
            .len()
            .checked_sub(self.cipher_type.tag_size())
            .ok_or(SecioError::FrameTooShort)?;
        let mut output = Vec::with_capacity(output_len);
        let mut buf = Vec::with_capacity(
            self.cipher_type
                .tag_size()
                .checked_add(input.len())
                .ok_or(SecioError::InvalidMessage)?,
        );

        unsafe {
            output.set_len(output_len);
            buf.set_len(input.len());
        }
        buf.copy_from_slice(input);

        if let RingAeadCryptoVariant::Open(ref mut key) = self.cipher {
            match key.open_in_place(Aad::empty(), &mut buf) {
                Ok(out_buf) => output.copy_from_slice(out_buf),
                Err(e) => return Err(e.into()),
            }
        } else {
            unreachable!("encrypt is called on a non-open cipher")
        }
        Ok(output)
    }
}

impl StreamCipher for RingAeadCipher {
    fn encrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        self.encrypt(input)
    }

    fn decrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        self.decrypt(input)
    }
}

#[cfg(test)]
mod test {
    use super::{CipherType, CryptoMode, RingAeadCipher};

    fn test_ring_aead(cipher: CipherType) {
        let key = (0..cipher.key_size()).map(|_| rand::random::<u8>()).collect::<Vec<_>>();

        // first time
        let message = b"HELLO WORLD";

        let mut enc = RingAeadCipher::new(cipher, &key[..], CryptoMode::Encrypt);

        let encrypted_msg = enc.encrypt(message).unwrap();

        assert_ne!(message, &encrypted_msg[..]);

        let mut dec = RingAeadCipher::new(cipher, &key[..], CryptoMode::Decrypt);
        let decrypted_msg = dec.decrypt(&encrypted_msg[..]).unwrap();

        assert_eq!(&decrypted_msg[..], message);

        // second time
        let message = b"hello, world";

        let encrypted_msg = enc.encrypt(message).unwrap();

        assert_ne!(message, &encrypted_msg[..]);

        let decrypted_msg = dec.decrypt(&encrypted_msg[..]).unwrap();

        assert_eq!(&decrypted_msg[..], message);
    }

    #[test]
    fn test_aes_128_gcm() {
        test_ring_aead(CipherType::Aes128Gcm)
    }

    #[test]
    fn test_aes_256_gcm() {
        test_ring_aead(CipherType::Aes256Gcm)
    }

    #[test]
    fn test_chacha20_poly1305() {
        test_ring_aead(CipherType::ChaCha20Poly1305)
    }
}
