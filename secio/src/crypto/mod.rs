use crate::error::SecioError;

/// Define cipher
pub mod cipher;
#[cfg(any(not(ossl110), test, not(unix)))]
mod ring_impl;

mod ctr_impl;

/// Variant cipher which contains all possible stream ciphers
#[doc(hidden)]
pub type BoxStreamCipher = Box<dyn StreamCipher + Send>;

/// Basic operation of Cipher, which is a Symmetric Cipher.
#[doc(hidden)]
pub trait StreamCipher {
    /// Feeds data from input through the cipher, return encrypted bytes.
    fn encrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError>;
    /// Feeds data from input through the cipher, return decrypted bytes.
    fn decrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError>;
}

/// Crypto mode, encrypt or decrypt
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[doc(hidden)]
pub enum CryptoMode {
    /// Encrypt
    Encrypt,
    /// Decrypt
    Decrypt,
}

/// Generate a specific Cipher with key and initialize vector
#[doc(hidden)]
pub fn new_stream(
    t: cipher::CipherType,
    key: &[u8],
    iv: &[u8],
    mode: CryptoMode,
) -> BoxStreamCipher {
    match t {
        cipher::CipherType::Aes128Ctr => {
            Box::new(ctr_impl::CTRCipher::new(t, key, iv))
        }
        cipher::CipherType::Aes128Gcm | cipher::CipherType::Aes256Gcm | cipher::CipherType::ChaCha20Poly1305 => {
            Box::new(ring_impl::RingAeadCipher::new(t, key, mode))
        }
    }
}

/// [0, 0, 0, 0]
/// [1, 0, 0, 0]
/// ...
/// [255, 0, 0, 0]
/// [0, 1, 0, 0]
/// [1, 1, 0, 0]
/// ...
fn nonce_advance(nonce: &mut [u8]) {
    for i in nonce {
        if std::u8::MAX == *i {
            *i = 0;
        } else {
            *i += 1;
            return;
        }
    }
}