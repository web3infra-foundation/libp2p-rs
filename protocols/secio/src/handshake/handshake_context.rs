/// Most of the code for this module comes from `rust-libp2p`, but it has been partially modified.
/// It does not use protobuf bytes when determining the order of the order. But the original public key bytes
use crate::{
    crypto::cipher::CipherType, error::SecioError, exchange::KeyAgreement, handshake_proto::Propose, support, Config, Digest,
};

use libp2prs_core::PublicKey;

use prost::Message;

use log::{debug, trace};
use ring::agreement;

use std::cmp::Ordering;

// This struct contains the whole context of a handshake, and is filled progressively
// throughout the various parts of the handshake.
pub struct HandshakeContext<T> {
    pub(crate) config: Config,
    pub(crate) state: T,
}

// HandshakeContext<()> --with_local-> HandshakeContext<Local>
pub struct Local {
    // Locally-generated random number. The array size can be changed without any repercussion.
    pub(crate) nonce: [u8; 16],
    // Our local public key bytes:
    pub(crate) public_key: Vec<u8>,
    // Our local proposition's raw bytes:
    pub(crate) proposition_bytes: Vec<u8>,
}

// HandshakeContext<Local> --with_remote-> HandshakeContext<Remote>
pub struct Remote {
    pub(crate) local: Local,
    // The remote's proposition's raw bytes:
    pub(crate) proposition_bytes: Vec<u8>,
    // The remote's public key:
    pub(crate) public_key: PublicKey,
    // The remote's `nonce`.
    // If the NONCE size is actually part of the protocol, we can change this to a fixed-size
    // array instead of a `Vec`.
    pub(crate) nonce: Vec<u8>,
    // Set to `ordering(
    //             hash(concat(remote-pubkey, local-none)),
    //             hash(concat(local-pubkey, remote-none))
    //         )`.
    // `Ordering::Equal` is an invalid value (as it would mean we're talking to ourselves).
    //
    // Since everything is symmetrical, this value is used to determine what should be ours
    // and what should be the remote's.
    pub(crate) hashes_ordering: Ordering,
    // Crypto algorithms chosen for the communication:
    pub(crate) chosen_exchange: KeyAgreement,
    pub(crate) chosen_cipher: CipherType,
    pub(crate) chosen_hash: Digest,
}

// HandshakeContext<Remote> --with_ephemeral-> HandshakeContext<Ephemeral>
pub struct Ephemeral {
    pub(crate) remote: Remote,
    // Ephemeral keypair generated for the handshake:
    pub(crate) local_tmp_priv_key: agreement::EphemeralPrivateKey,
    pub(crate) local_tmp_pub_key: Vec<u8>,
}

// HandshakeContext<Ephemeral> --take_private_key-> HandshakeContext<PubEphemeral>
pub struct PubEphemeral {
    pub(crate) remote: Remote,
    pub(crate) local_tmp_pub_key: Vec<u8>,
}

impl HandshakeContext<()> {
    pub fn new(config: Config) -> Self {
        HandshakeContext { config, state: () }
    }

    // Setup local proposition.
    pub fn with_local(self) -> HandshakeContext<Local> {
        let nonce: [u8; 16] = rand::random();

        let public_key = self.config.key.public();

        let local_proposition = Propose {
            rand: nonce.to_vec(),
            pubkey: public_key.clone().into_protobuf_encoding(),
            exchanges: self
                .config
                .agreements_proposal
                .clone()
                .unwrap_or_else(|| support::DEFAULT_AGREEMENTS_PROPOSITION.into()),
            ciphers: self
                .config
                .ciphers_proposal
                .clone()
                .unwrap_or_else(|| support::DEFAULT_CIPHERS_PROPOSITION.into()),
            hashes: self
                .config
                .digests_proposal
                .clone()
                .unwrap_or_else(|| support::DEFAULT_DIGESTS_PROPOSITION.into()),
        };

        let proposition_bytes = {
            let mut buf = Vec::with_capacity(local_proposition.encoded_len());
            local_proposition.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
            buf
        };

        HandshakeContext {
            config: self.config,
            state: Local {
                nonce,
                public_key: public_key.into_protobuf_encoding(),
                proposition_bytes,
            },
        }
    }
}

impl HandshakeContext<Local> {
    // Process remote proposition.
    pub fn with_remote(self, remote_bytes: Vec<u8>) -> Result<HandshakeContext<Remote>, SecioError> {
        let propose = match Propose::decode(&remote_bytes[..]) {
            Ok(prop) => prop,
            Err(_) => {
                debug!("failed to parse remote's proposition protobuf message");
                return Err(SecioError::HandshakeParsingFailure);
            }
        };

        // NOTE: Libp2p uses protobuf bytes to calculate order, but here we only use the original pubkey and nonce
        let nonce = propose.rand;

        let public_key = propose.pubkey;
        trace!("remote public_key: {:?}", &public_key);

        if public_key == self.state.public_key {
            return Err(SecioError::ConnectSelf);
        }

        // In order to determine which protocols to use, we compute two hashes and choose
        // based on which hash is larger.
        let hashes_ordering = {
            let oh1 = {
                let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
                ctx.update(public_key.as_ref());
                ctx.update(&self.state.nonce);
                ctx.finish()
            };

            let oh2 = {
                let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
                ctx.update(&self.state.public_key);
                ctx.update(&nonce);
                ctx.finish()
            };

            oh1.as_ref().cmp(&oh2.as_ref())
        };

        let chosen_exchange = {
            let ours = self
                .config
                .agreements_proposal
                .as_ref()
                .map(AsRef::as_ref)
                .unwrap_or(support::DEFAULT_AGREEMENTS_PROPOSITION);
            let theirs = &propose.exchanges;
            match support::select_agreement(hashes_ordering, ours, theirs) {
                Ok(a) => a,
                Err(err) => {
                    debug!("failed to select an exchange protocol");
                    return Err(err);
                }
            }
        };

        let chosen_cipher = {
            let ours = self
                .config
                .ciphers_proposal
                .as_ref()
                .map(AsRef::as_ref)
                .unwrap_or(support::DEFAULT_CIPHERS_PROPOSITION);
            let theirs = &propose.ciphers;
            match support::select_cipher(hashes_ordering, ours, theirs) {
                Ok(a) => {
                    debug!("selected cipher: {:?}", a);
                    a
                }
                Err(err) => {
                    debug!("failed to select a cipher protocol");
                    return Err(err);
                }
            }
        };

        let chosen_hash = {
            let ours = self
                .config
                .digests_proposal
                .as_ref()
                .map(AsRef::as_ref)
                .unwrap_or(support::DEFAULT_DIGESTS_PROPOSITION);
            let theirs = &propose.hashes;
            match support::select_digest(hashes_ordering, ours, theirs) {
                Ok(a) => {
                    debug!("selected hash: {:?}", a);
                    a
                }
                Err(err) => {
                    debug!("failed to select a hash protocol");
                    return Err(err);
                }
            }
        };

        let public_key = match PublicKey::from_protobuf_encoding(public_key.as_ref()) {
            Ok(key) => key,
            Err(_e) => {
                return Err(SecioError::HandshakeParsingFailure);
            }
        };

        Ok(HandshakeContext {
            config: self.config,
            state: Remote {
                local: self.state,
                proposition_bytes: remote_bytes,
                public_key,
                nonce,
                hashes_ordering,
                chosen_exchange,
                chosen_cipher,
                chosen_hash,
            },
        })
    }
}

impl HandshakeContext<Remote> {
    pub fn with_ephemeral(self, sk: agreement::EphemeralPrivateKey, pk: Vec<u8>) -> HandshakeContext<Ephemeral> {
        HandshakeContext {
            config: self.config,
            state: Ephemeral {
                remote: self.state,
                local_tmp_priv_key: sk,
                local_tmp_pub_key: pk,
            },
        }
    }
}

impl HandshakeContext<Ephemeral> {
    pub fn take_private_key(self) -> (HandshakeContext<PubEphemeral>, agreement::EphemeralPrivateKey) {
        let context = HandshakeContext {
            config: self.config,
            state: PubEphemeral {
                remote: self.state.remote,
                local_tmp_pub_key: self.state.local_tmp_pub_key,
            },
        };
        (context, self.state.local_tmp_priv_key)
    }
}
