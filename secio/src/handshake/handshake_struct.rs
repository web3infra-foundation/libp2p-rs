use crate::peer_id::PeerId;
use crate::key_proto;

use std::fmt;

/// Public Key
#[derive(Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum PublicKey {
    /// Secp256k1
    Secp256k1(Vec<u8>),
}

impl PublicKey {
    /// Get inner data
    pub fn inner_ref(&self) -> &Vec<u8> {
        match self {
            PublicKey::Secp256k1(ref key) => key,
        }
    }

    /// Get inner data
    pub fn inner(self) -> Vec<u8> {
        match self {
            PublicKey::Secp256k1(key) => key,
        }
    }

    /// Creates a public key directly from a slice
    pub fn secp256k1_raw_key<K>(key: K) -> Result<Self, crate::error::SecioError>
        where
            K: AsRef<[u8]>,
    {
        secp256k1::key::PublicKey::from_slice(key.as_ref())
            .map(|key| PublicKey::Secp256k1(key.serialize().to_vec()))
            .map_err(|_| crate::error::SecioError::SecretGenerationFailed)
    }

    /// Get protobuf data
    pub fn into_protobuf_encoding(self) -> Vec<u8> {
        use prost::Message;

        let public_key = match self {
            PublicKey::Secp256k1(key) =>
                key_proto::PublicKey {
                    r#type: key_proto::KeyType::Secp256k1 as i32,
                    data: key,
                }
        };
        let mut buf = Vec::with_capacity(public_key.encoded_len());
        public_key.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        buf
    }

    /// Generate Peer id
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_public_key(self)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x")?;
        for byte in self.inner_ref() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}
