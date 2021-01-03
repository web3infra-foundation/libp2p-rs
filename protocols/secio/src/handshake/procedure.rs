/// Most of the code for this module comes from `rust-libp2p` and `go-libp2p`.
///
/// Some panic logic has been removed, some error handling has been removed, and an error has been added.
///
use log::{debug, trace};
use std::cmp::Ordering;

use crate::{
    codec::{secure_stream::SecureStream, Hmac},
    crypto::{cipher::CipherType, new_stream, BoxStreamCipher, CryptoMode},
    error::SecioError,
    exchange,
    handshake::handshake_context::HandshakeContext,
    handshake_proto::Exchange,
    Config, Digest, EphemeralPublicKey,
};

use libp2prs_core::identity::*;
use libp2prs_core::PublicKey;

use libp2prs_traits::{ReadEx, SplittableReadWrite, WriteEx};
use prost::Message;

/// Performs a handshake on the given socket.
///
/// This function expects that the remote is identified with `remote_public_key`, and the remote
/// will expect that we are identified with `local_key`.Any mismatch somewhere will produce a
/// `SecioError`.
///
/// On success, returns an object that implements the `WriteEx` and `ReadEx` trait,
/// plus the public key of the remote, plus the ephemeral public key used during
/// negotiation.
pub(crate) async fn handshake<T>(
    socket: T,
    config: Config,
) -> Result<(SecureStream<T::Reader, T::Writer>, PublicKey, EphemeralPublicKey), SecioError>
where
    T: SplittableReadWrite,
{
    let max_frame_len = config.max_frame_length;
    // The handshake messages all start with a 4-bytes message length prefix.
    // let mut socket = LengthPrefixSocket::new(socket, config.max_frame_length);
    let (mut reader, mut writer) = socket.split();

    // Generate our nonce.
    let local_context = HandshakeContext::new(config).with_local();
    trace!("starting handshake; local nonce = {:?}", local_context.state.nonce);

    trace!("sending proposition to remote");
    writer.write_one_fixed(&local_context.state.proposition_bytes).await?;

    // Receive the remote's proposition.
    let remote_proposition = reader.read_one_fixed(max_frame_len).await?;
    let remote_context = local_context.with_remote(remote_proposition)?;

    trace!(
        "received proposition from remote; pubkey = {:?}; nonce = {:?}",
        remote_context.state.public_key,
        remote_context.state.nonce
    );

    // Generate an ephemeral key for the negotiation.
    let (tmp_priv_key, tmp_pub_key) = exchange::generate_agreement(remote_context.state.chosen_exchange)?;

    // Send the ephemeral pub key to the remote in an `Exchange` struct. The `Exchange` also
    // contains a signature of the two propositions encoded with our static public key.
    let ephemeral_context = remote_context.with_ephemeral(tmp_priv_key, tmp_pub_key.clone());
    let exchanges = {
        let mut data_to_sign = ephemeral_context.state.remote.local.proposition_bytes.clone();

        data_to_sign.extend_from_slice(&ephemeral_context.state.remote.proposition_bytes);
        data_to_sign.extend_from_slice(&tmp_pub_key);

        let kpair: Keypair = ephemeral_context.config.key.clone();
        let signature = match kpair.sign(data_to_sign.as_ref()) {
            Ok(signature) => signature,
            Err(_e) => {
                return Err(SecioError::HandshakeParsingFailure);
            }
        };

        Exchange {
            epubkey: tmp_pub_key.clone(),
            signature,
        }
    };
    let local_exchanges = {
        let mut buf = Vec::with_capacity(exchanges.encoded_len());
        exchanges.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        buf
    };

    // Send our local `Exchange`.
    trace!("sending exchange to remote");

    writer.write_one_fixed(&local_exchanges).await?;

    // Receive the remote's `Exchange`.
    let raw_exchanges = reader.read_one_fixed(max_frame_len).await?;
    let remote_exchanges = match Exchange::decode(&raw_exchanges[..]) {
        Ok(e) => e,
        Err(err) => {
            debug!("failed to parse remote's exchange protobuf; {:?}", err);
            return Err(SecioError::HandshakeParsingFailure);
        }
    };

    trace!("received and decoded the remote's exchange");

    // Check the validity of the remote's `Exchange`. This verifies that the remote was really
    // the sender of its proposition, and that it is the owner of both its global and ephemeral
    // keys.
    let mut data_to_verify = ephemeral_context.state.remote.proposition_bytes.clone();
    data_to_verify.extend_from_slice(&ephemeral_context.state.remote.local.proposition_bytes);
    data_to_verify.extend_from_slice(&remote_exchanges.epubkey);

    let remote_public_key = ephemeral_context.state.remote.public_key.clone();

    if !remote_public_key.verify(data_to_verify.as_ref(), remote_exchanges.signature.as_ref()) {
        debug!("failed to verify the remote's signature");
        return Err(SecioError::SignatureVerificationFailed);
    }

    trace!("successfully verified the remote's signature");

    // Generate a key from the local ephemeral private key and the remote ephemeral public key,
    // derive from it a cipher key, an iv, and a hmac key, and build the encoder/decoder.
    let (pub_ephemeral_context, local_priv_key) = ephemeral_context.take_private_key();
    let key_material = exchange::agree(
        pub_ephemeral_context.state.remote.chosen_exchange,
        local_priv_key,
        &remote_exchanges.epubkey,
    )?;

    // Generate a key from the local ephemeral private key and the remote ephemeral public key,
    // derive from it a cipher key, an iv, and a hmac key, and build the encoder/decoder.
    let chosen_cipher = pub_ephemeral_context.state.remote.chosen_cipher;
    let cipher_key_size = chosen_cipher.key_size();
    let iv_size = chosen_cipher.iv_size();

    let key = Hmac::from_key(pub_ephemeral_context.state.remote.chosen_hash, &key_material);
    let mut longer_key = vec![0u8; 2 * (iv_size + cipher_key_size + 20)];
    stretch_key(key, &mut longer_key);

    let (local_infos, remote_infos) = {
        let (first_half, second_half) = longer_key.split_at(longer_key.len() / 2);
        match pub_ephemeral_context.state.remote.hashes_ordering {
            Ordering::Equal => {
                let msg = "equal digest of public key and nonce for local and remote";
                return Err(SecioError::InvalidProposition(msg));
            }
            Ordering::Less => (second_half, first_half),
            Ordering::Greater => (first_half, second_half),
        }
    };

    trace!("local info: {:?}, remote_info: {:?}", local_infos, remote_infos);

    let (encode_cipher, encode_hmac) = generate_stream_cipher_and_hmac(
        chosen_cipher,
        pub_ephemeral_context.state.remote.chosen_hash,
        CryptoMode::Encrypt,
        local_infos,
        cipher_key_size,
        iv_size,
    );

    let (decode_cipher, decode_hmac) = generate_stream_cipher_and_hmac(
        chosen_cipher,
        pub_ephemeral_context.state.remote.chosen_hash,
        CryptoMode::Decrypt,
        remote_infos,
        cipher_key_size,
        iv_size,
    );

    let mut secure_stream = SecureStream::new(
        reader,
        writer,
        max_frame_len,
        decode_cipher,
        decode_hmac,
        encode_cipher,
        encode_hmac,
        pub_ephemeral_context.state.remote.local.nonce.to_vec(),
    );

    // We send back their nonce to check if the connection works.
    trace!("checking encryption by sending back remote's nonce");
    secure_stream.write2(&pub_ephemeral_context.state.remote.nonce).await?;
    secure_stream.verify_nonce().await?;

    trace!(
        "handshake for {:?} successfully done!",
        pub_ephemeral_context.state.remote.local.nonce
    );

    Ok((
        secure_stream,
        pub_ephemeral_context.state.remote.public_key,
        pub_ephemeral_context.state.local_tmp_pub_key,
    ))
}

/// Custom algorithm translated from reference implementations. Needs to be the same algorithm
/// amongst all implementations.
fn stretch_key(hmac: Hmac, result: &mut [u8]) {
    const SEED: &[u8] = b"key expansion";

    let mut init_ctxt = hmac.context();
    init_ctxt.update(SEED);
    let mut a = init_ctxt.sign();

    let mut j = 0;
    while j < result.len() {
        let mut context = hmac.context();
        context.update(a.as_ref());
        context.update(SEED);
        let b = context.sign();

        let todo = ::std::cmp::min(b.as_ref().len(), result.len() - j);

        result[j..j + todo].copy_from_slice(&b.as_ref()[..todo]);

        j += todo;

        let mut context = hmac.context();
        context.update(a.as_ref());
        a = context.sign();
    }
}

fn generate_stream_cipher_and_hmac(
    t: CipherType,
    _digest: Digest,
    mode: CryptoMode,
    info: &[u8],
    key_size: usize,
    iv_size: usize,
) -> (BoxStreamCipher, Option<Hmac>) {
    let (iv, rest) = info.split_at(iv_size);
    let (cipher_key, _mac_key) = rest.split_at(key_size);
    let hmac = match t {
        CipherType::ChaCha20Poly1305 | CipherType::Aes128Gcm | CipherType::Aes256Gcm => None,
        _ => Some(Hmac::from_key(_digest, _mac_key)),
    };
    let cipher = new_stream(t, cipher_key, iv, mode);
    (cipher, hmac)
}

#[cfg(test)]
mod tests {
    use super::stretch_key;
    use crate::{codec::Hmac, Config, Digest};

    use async_std::task;
    use bytes::BytesMut;
    use futures::channel;
    //use futures::prelude::*;
    use libp2prs_core::identity::Keypair;
    use libp2prs_traits::{ReadEx, WriteEx};

    fn handshake_with_self_success(config_1: Config, config_2: Config, data: &'static [u8]) {
        let (sender, receiver) = channel::oneshot::channel::<bytes::BytesMut>();
        let (addr_sender, addr_receiver) = channel::oneshot::channel::<::std::net::SocketAddr>();

        task::spawn(async move {
            let listener = async_std::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let listener_addr = listener.local_addr().unwrap();
            let _res = addr_sender.send(listener_addr);
            let (connect, _) = listener.accept().await.unwrap();
            let (mut handle, _, _) = config_1.handshake(connect).await.unwrap();
            let mut data = [0u8; 11];
            handle.read2(&mut data).await.unwrap();
            handle.write2(&data).await.unwrap();
        });

        task::spawn(async move {
            let listener_addr = addr_receiver.await.unwrap();
            let connect = async_std::net::TcpStream::connect(&listener_addr).await.unwrap();
            let (mut handle, _, _) = config_2.handshake(connect).await.unwrap();
            handle.write2(data).await.unwrap();
            let mut data = [0u8; 11];
            handle.read2(&mut data).await.unwrap();
            let _res = sender.send(BytesMut::from(&data[..]));
        });

        task::block_on(async move {
            let received = receiver.await.unwrap();
            assert_eq!(received.to_vec(), data);
        });
    }

    #[test]
    fn handshake_with_self_success_secp256k1_small_data() {
        let key_1 = Keypair::generate_secp256k1();
        let key_2 = Keypair::generate_secp256k1();
        handshake_with_self_success(Config::new(key_1), Config::new(key_2), b"hello world")
    }

    #[test]
    fn stretch() {
        let mut output = [0u8; 32];

        let key1 = Hmac::from_key(Digest::Sha256, &[]);
        stretch_key(key1, &mut output);
        assert_eq!(
            &output,
            &[
                103, 144, 60, 199, 85, 145, 239, 71, 79, 198, 85, 164, 32, 53, 143, 205, 50, 48, 153, 10, 37, 32, 85, 1, 226, 61, 193,
                1, 154, 120, 207, 80,
            ]
        );

        let key2 = Hmac::from_key(
            Digest::Sha256,
            &[
                157, 166, 80, 144, 77, 193, 198, 6, 23, 220, 87, 220, 191, 72, 168, 197, 54, 33, 219, 225, 84, 156, 165, 37, 149, 224,
                244, 32, 170, 79, 125, 35, 171, 26, 178, 176, 92, 168, 22, 27, 205, 44, 229, 61, 152, 21, 222, 81, 241, 81, 116, 236,
                74, 166, 89, 145, 5, 162, 108, 230, 55, 54, 9, 17,
            ],
        );
        stretch_key(key2, &mut output);
        assert_eq!(
            &output,
            &[
                39, 151, 182, 63, 180, 175, 224, 139, 42, 131, 130, 116, 55, 146, 62, 31, 157, 95, 217, 15, 73, 81, 10, 83, 243, 141,
                64, 227, 103, 144, 99, 121,
            ]
        );

        let key3 = Hmac::from_key(
            Digest::Sha256,
            &[
                98, 219, 94, 104, 97, 70, 139, 13, 185, 110, 56, 36, 66, 3, 80, 224, 32, 205, 102, 170, 59, 32, 140, 245, 86, 102,
                231, 68, 85, 249, 227, 243, 57, 53, 171, 36, 62, 225, 178, 74, 89, 142, 151, 94, 183, 231, 208, 166, 244, 130, 130,
                209, 248, 65, 19, 48, 127, 127, 55, 82, 117, 154, 124, 108,
            ],
        );
        stretch_key(key3, &mut output);
        assert_eq!(
            &output,
            &[
                28, 39, 158, 206, 164, 16, 211, 194, 99, 43, 208, 36, 24, 141, 90, 93, 157, 236, 238, 111, 170, 0, 60, 11, 49, 174,
                177, 121, 30, 12, 182, 25,
            ]
        );
    }
}
