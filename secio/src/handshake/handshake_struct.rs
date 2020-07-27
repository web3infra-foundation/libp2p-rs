#[cfg(feature = "flatc")]
use crate::handshake::handshake_generated::p2p::handshake::{
    Exchange as FBSExchange, ExchangeBuilder, Propose as FBSPropose, ProposeBuilder,
    PublicKey as FBSPublicKey, PublicKeyBuilder, Type,
};

use crate::handshake::handshake_mol;
use molecule::prelude::{Builder, Entity, Reader};

use crate::peer_id::PeerId;

use bytes::Bytes;
use std::fmt;

#[derive(Clone, Default, PartialEq, Ord, PartialOrd, Eq, Debug)]
pub struct Propose {
    pub(crate) rand: Vec<u8>,
    /// flatbuffer public key bytes
    pub(crate) pubkey: Bytes,
    pub(crate) exchange: String,
    pub(crate) ciphers: String,
    pub(crate) hashes: String,
}

impl Propose {
    pub fn new() -> Self {
        Default::default()
    }

    /// Encode with flatbuffer
    #[cfg(feature = "flatc")]
    pub fn encode(&self) -> Bytes {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let rand = fbb.create_vector(&self.rand);
        let pub_key = fbb.create_vector(&self.pubkey);
        let exchange = fbb.create_string(&self.exchange);
        let ciphers = fbb.create_string(&self.ciphers);
        let hashes = fbb.create_string(&self.hashes);

        let mut builder = ProposeBuilder::new(&mut fbb);
        builder.add_rand(rand);
        builder.add_pubkey(pub_key);
        builder.add_exchanges(exchange);
        builder.add_ciphers(ciphers);
        builder.add_hashes(hashes);
        let data = builder.finish();

        fbb.finish(data, None);
        Bytes::from(fbb.finished_data().to_owned())
    }

    /// Decode with Flatbuffer
    #[cfg(feature = "flatc")]
    pub fn decode(data: &[u8]) -> Option<Self> {
        let fbs_propose = flatbuffers_verifier::get_root::<FBSPropose>(data).ok()?;
        match (
            fbs_propose.rand(),
            fbs_propose.pubkey(),
            fbs_propose.exchanges(),
            fbs_propose.ciphers(),
            fbs_propose.hashes(),
        ) {
            (Some(rand), Some(pubkey), Some(exchange), Some(ciphers), Some(hashes)) => {
                Some(Propose {
                    rand: rand.to_owned(),
                    pubkey: Bytes::from(pubkey.to_owned()),
                    exchange: exchange.to_owned(),
                    ciphers: ciphers.to_owned(),
                    hashes: hashes.to_owned(),
                })
            }
            _ => None,
        }
    }

    /// Encode with molecule
    pub fn encode(self) -> Bytes {
        let rand = handshake_mol::Bytes::new_builder()
            .set(self.rand.into_iter().map(Into::into).collect())
            .build();
        let pubkey = handshake_mol::Bytes::new_builder()
            .set(self.pubkey.to_vec().into_iter().map(Into::into).collect())
            .build();
        let exchange = handshake_mol::String::new_builder()
            .set(
                self.exchange
                    .into_bytes()
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
            .build();
        let ciphers = handshake_mol::String::new_builder()
            .set(
                self.ciphers
                    .into_bytes()
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
            .build();
        let hashes = handshake_mol::String::new_builder()
            .set(
                self.hashes
                    .into_bytes()
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
            .build();

        handshake_mol::Propose::new_builder()
            .rand(rand)
            .pubkey(pubkey)
            .exchanges(exchange)
            .ciphers(ciphers)
            .hashes(hashes)
            .build()
            .as_bytes()
    }

    /// Decode with molecule
    pub fn decode(data: &[u8]) -> Option<Self> {
        let reader = handshake_mol::ProposeReader::from_compatible_slice(data).ok()?;
        Some(Propose {
            rand: reader.rand().raw_data().to_owned(),
            pubkey: Bytes::from(reader.pubkey().raw_data().to_owned()),
            exchange: String::from_utf8(reader.exchanges().raw_data().to_owned()).ok()?,
            ciphers: String::from_utf8(reader.ciphers().raw_data().to_owned()).ok()?,
            hashes: String::from_utf8(reader.hashes().raw_data().to_owned()).ok()?,
        })
    }
}

#[derive(Clone, Default, PartialEq, Ord, PartialOrd, Eq, Debug)]
pub struct Exchange {
    pub(crate) epubkey: Vec<u8>,
    pub(crate) signature: Vec<u8>,
}

impl Exchange {
    pub fn new() -> Self {
        Default::default()
    }

    /// Encode with molecule
    pub fn encode(self) -> Bytes {
        let epubkey = handshake_mol::Bytes::new_builder()
            .set(self.epubkey.into_iter().map(Into::into).collect())
            .build();
        let signature = handshake_mol::Bytes::new_builder()
            .set(self.signature.into_iter().map(Into::into).collect())
            .build();

        handshake_mol::Exchange::new_builder()
            .epubkey(epubkey)
            .signature(signature)
            .build()
            .as_bytes()
    }

    /// Decode with molecule
    pub fn decode(data: &[u8]) -> Option<Self> {
        let reader = handshake_mol::ExchangeReader::from_compatible_slice(data).ok()?;
        Some(Exchange {
            epubkey: reader.epubkey().raw_data().to_owned(),
            signature: reader.signature().raw_data().to_owned(),
        })
    }
}

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

    /// Encode with flatbuffer
    #[cfg(feature = "flatc")]
    pub fn encode(&self) -> Bytes {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let pubkey = fbb.create_vector(self.inner_ref());

        let mut builder = PublicKeyBuilder::new(&mut fbb);
        builder.add_key_type(Type::Secp256k1);
        builder.add_pubkey(pubkey);

        let data = builder.finish();

        fbb.finish(data, None);
        Bytes::from(fbb.finished_data().to_owned())
    }

    /// Decode with Flatbuffer
    #[cfg(feature = "flatc")]
    pub fn decode(data: &[u8]) -> Option<Self> {
        let pubkey = flatbuffers_verifier::get_root::<FBSPublicKey>(data).ok()?;
        match pubkey.pubkey() {
            Some(pub_key) => match pubkey.key_type() {
                Type::Secp256k1 => Some(PublicKey::Secp256k1(pub_key.to_owned())),
            },
            None => None,
        }
    }

    /// Encode with molecule
    pub fn encode(self) -> Bytes {
        let secp256k1 = handshake_mol::Secp256k1::new_builder()
            .set(self.inner().into_iter().map(Into::into).collect())
            .build();
        let pubkey = handshake_mol::PublicKey::new_builder()
            .set(secp256k1)
            .build();
        pubkey.as_bytes()
    }

    /// Decode with molecule
    pub fn decode(data: &[u8]) -> Option<Self> {
        let reader = handshake_mol::PublicKeyReader::from_compatible_slice(data).ok()?;
        let union = reader.to_enum();
        match union {
            handshake_mol::PublicKeyUnionReader::Secp256k1(reader) => {
                Some(PublicKey::Secp256k1(reader.raw_data().to_owned()))
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{Exchange, Propose, PublicKey};
    use crate::SecioKeyPair;
    use bytes::Bytes;

    #[test]
    fn decode_encode_pubkey() {
        let raw = SecioKeyPair::secp256k1_generated().public_key();
        let byte = raw.clone().encode();

        assert_eq!(raw, PublicKey::decode(&byte).unwrap())
    }

    #[test]
    fn decode_encode_propose() {
        let nonce: [u8; 16] = rand::random();
        let mut raw = Propose::new();
        raw.rand = nonce.to_vec();
        raw.pubkey = Bytes::from(vec![25u8; 256]);

        let byte = raw.clone().encode();

        assert_eq!(raw, Propose::decode(&byte).unwrap())
    }

    #[test]
    fn decode_encode_exchange() {
        let mut raw = Exchange::new();
        raw.signature = vec![1u8; 256];
        raw.epubkey = vec![9u8; 256];

        let byte = raw.clone().encode();

        assert_eq!(raw, Exchange::decode(&byte).unwrap())
    }

    #[test]
    fn test_pubkey_from_slice() {
        let privkey = SecioKeyPair::secp256k1_generated();
        let raw = privkey.public_key();
        let inner = raw.inner_ref();

        let other = PublicKey::secp256k1_raw_key(inner).unwrap();
        assert_eq!(raw, other);
        let uncompressed = secp256k1::key::PublicKey::from_slice(inner)
            .map(|key| key.serialize_uncompressed().to_vec())
            .unwrap();

        let other_1 = PublicKey::secp256k1_raw_key(uncompressed).unwrap();
        assert_eq!(raw, other_1);
    }
}
