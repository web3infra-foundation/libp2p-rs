use crate::crypto::StreamCipher;

use crate::error::SecioError;

use aes_ctr::stream_cipher::generic_array::*;
use aes_ctr::stream_cipher::{NewStreamCipher, SyncStreamCipher};
use aes_ctr::Aes128Ctr;
use log::trace;

pub static CTR128LEN: usize = 16;

pub(crate) struct CTRCipher {
    cipher: Aes128Ctr,
}

impl CTRCipher {
    /// Create a CTRCipher
    pub fn new(key: &[u8], iv: &[u8]) -> Self {
        trace!("new CTRCipher");
        let iv = GenericArray::from_slice(iv);
        let key = GenericArray::from_slice(key);
        let cipher = Aes128Ctr::new(key, iv);
        CTRCipher { cipher }
    }

    /// Encrypt `input` to `temp`, tag.len() in ctr mode is zero.
    pub fn encrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        let mut output = Vec::from(input);
        self.cipher.apply_keystream(&mut output);
        Ok(output)
    }

    /// Decrypt `input` to `output`, tag.len() in ctr mode is zero.
    pub fn decrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        let mut output = Vec::from(input);
        self.cipher.apply_keystream(&mut output);
        Ok(output)
    }
}

impl StreamCipher for CTRCipher {
    fn encrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        self.encrypt(input)
    }

    fn decrypt(&mut self, input: &[u8]) -> Result<Vec<u8>, SecioError> {
        self.decrypt(input)
    }
}

#[cfg(test)]
mod test {
    use crate::crypto::cipher::CipherType;
    use crate::crypto::ctr_impl::CTRCipher;

    fn test_ctr(mode: CipherType) {
        let key = (0..mode.key_size())
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>();
        let iv = (0..mode.iv_size())
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>();

        let mut encryptor = CTRCipher::new(key.as_ref(), iv.as_ref());
        let mut decryptor = CTRCipher::new(key.as_ref(), iv.as_ref());

        let message = b"HELLO WORLD";
        let encrypted_msg = encryptor.encrypt(message).unwrap();
        let decrypted_msg = decryptor.decrypt(&encrypted_msg[..]).unwrap();
        assert_eq!(&decrypted_msg[..], message);

        let message = b"hello world";
        let encrypted_msg = encryptor.encrypt(message).unwrap();
        let decrypted_msg = decryptor.decrypt(&encrypted_msg[..]).unwrap();
        assert_eq!(&decrypted_msg[..], message);
    }

    #[test]
    fn test_aes_128_ctr() {
        test_ctr(CipherType::Aes128Ctr);
    }
}
