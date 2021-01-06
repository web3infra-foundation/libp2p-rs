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

//! Noise protocol handshake I/O.

mod payload_proto {
    include!(concat!(env!("OUT_DIR"), "/payload.proto.rs"));
}

use crate::error::NoiseError;
use crate::io::{framed::NoiseFramed, NoiseOutput};
use crate::protocol::{KeypairIdentity, Protocol, PublicKey};
use bytes::Bytes;
use libp2prs_core::identity;
use libp2prs_traits::SplittableReadWrite;
use log::info;
use prost::Message;
use std::io;

/// The identity of the remote established during a handshake.
pub enum RemoteIdentity<C> {
    /// The remote provided no identifying information.
    ///
    /// The identity of the remote is unknown and must be obtained through
    /// a different, out-of-band channel.
    Unknown,

    /// The remote provided a static DH public key.
    ///
    /// The static DH public key is authentic in the sense that a successful
    /// handshake implies that the remote possesses a corresponding secret key.
    ///
    /// > **Note**: To rule out active attacks like a MITM, trust in the public key must
    /// > still be established, e.g. by comparing the key against an expected or
    /// > otherwise known public key.
    StaticDhKey(PublicKey<C>),

    /// The remote provided a public identity key in addition to a static DH
    /// public key and the latter is authentic w.r.t. the former.
    ///
    /// > **Note**: To rule out active attacks like a MITM, trust in the public key must
    /// > still be established, e.g. by comparing the key against an expected or
    /// > otherwise known public key.
    IdentityKey(identity::PublicKey),
}

/// The options for identity exchange in an authenticated handshake.
///
/// > **Note**: Even if a remote's public identity key is known a priori,
/// > unless the authenticity of the key is [linked](Protocol::linked) to
/// > the authenticity of a remote's static DH public key, an authenticated
/// > handshake will still send the associated signature of the provided
/// > local [`KeypairIdentity`] in order for the remote to verify that the static
/// > DH public key is authentic w.r.t. the known public identity key.
pub enum IdentityExchange {
    /// Send the local public identity to the remote.
    ///
    /// The remote identity is unknown (i.e. expected to be received).
    Mutual,
    /// Send the local public identity to the remote.
    ///
    /// The remote identity is known.
    Send { remote: identity::PublicKey },
    /// Don't send the local public identity to the remote.
    ///
    /// The remote identity is unknown, i.e. expected to be received.
    Receive,
    /// Don't send the local public identity to the remote.
    ///
    /// The remote identity is known, thus identities must be mutually known
    /// in order for the handshake to succeed.
    None { remote: identity::PublicKey },
}

/// Creates an authenticated Noise handshake for the initiator of a
/// 1.5-roundtrip (3 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence expects
/// the remote to identify itself in the second message payload and
/// identifies the local node to the remote in the third message payload.
/// The first (unencrypted) message payload is always empty.
///
/// This message sequence is suitable for authenticated 3-message Noise handshake
/// patterns where the static keys of the responder and initiator are either known
/// (i.e. appear in the pre-message pattern) or are sent with the second and third
/// message, respectively (e.g. `XX`).
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```
pub async fn initiator<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    priv_key: identity::Keypair,
) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
where
    T: SplittableReadWrite,
    C: Protocol<C> + AsRef<[u8]>,
{
    let mut state = State::new(io, session, identity, identity_x)?;
    send_empty(&mut state).await?;
    info!("send empty finished");
    recv_identity(&mut state).await?;
    info!("recv identity finished");
    send_identity(&mut state).await?;
    state.finish(priv_key)
}

/// Creates an authenticated Noise handshake for the responder of a
/// 1.5-roundtrip (3 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence
/// identifies the local node in the second message payload and expects
/// the remote to identify itself in the third message payload. The first
/// (unencrypted) message payload is always empty.
///
/// This message sequence is suitable for authenticated 3-message Noise handshake
/// patterns where the static keys of the responder and initiator are either known
/// (i.e. appear in the pre-message pattern) or are sent with the second and third
/// message, respectively (e.g. `XX`).
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```
pub async fn responder<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    keypair: identity::Keypair,
) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
where
    T: SplittableReadWrite,
    C: Protocol<C> + AsRef<[u8]>,
{
    let mut state = State::new(io, session, identity, identity_x)?;
    recv_empty(&mut state).await?;
    info!("recv_empty finished");
    send_identity(&mut state).await?;
    recv_identity(&mut state).await?;
    state.finish(keypair)
}

/// Handshake state.
struct State<T> {
    /// The underlying I/O resource.
    io: NoiseFramed<T>,
    /// The associated public identity of the local node's static DH keypair,
    /// which can be sent to the remote as part of an authenticated handshake.
    identity: KeypairIdentity,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
    /// Whether to send the public identity key of the local node to the remote.
    send_identity: bool,
}

impl<T: SplittableReadWrite> State<T> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.
    fn new(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange,
    ) -> Result<Self, NoiseError> {
        let (id_remote_pubkey, send_identity) = match identity_x {
            IdentityExchange::Mutual => (None, true),
            IdentityExchange::Send { remote } => (Some(remote), true),
            IdentityExchange::Receive => (None, false),
            IdentityExchange::None { remote } => (Some(remote), false),
        };
        session.map(|s| State {
            identity,
            io: NoiseFramed::new(io, s),
            dh_remote_pubkey_sig: None,
            id_remote_pubkey,
            send_identity,
        })
    }

    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    fn finish<C>(self, keypair: identity::Keypair) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        C: Protocol<C> + AsRef<[u8]>,
    {
        let (pubkey, mut io) = self.io.into_transport(keypair)?;
        let remote = match (self.id_remote_pubkey, pubkey) {
            (_, None) => RemoteIdentity::Unknown,
            (None, Some(dh_pk)) => RemoteIdentity::StaticDhKey(dh_pk),
            (Some(id_pk), Some(dh_pk)) => {
                let pubkey = id_pk.clone();
                io.remote_pub_key = pubkey;
                if C::verify(&id_pk, &dh_pk, &self.dh_remote_pubkey_sig) {
                    RemoteIdentity::IdentityKey(id_pk)
                } else {
                    return Err(NoiseError::InvalidKey);
                }
            }
        };

        Ok((remote, io))
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message

/// Using async/await to receive a Noise handshake message.
async fn recv<T>(state: &mut State<T>) -> Result<Bytes, NoiseError>
where
    T: SplittableReadWrite,
{
    match state.io.next().await {
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof").into()),
        Some(Err(e)) => Err(e),
        Some(Ok(m)) => Ok(m),
    }
}

/// Using async/await to receive a Noise handshake message with an empty payload.
async fn recv_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: SplittableReadWrite,
{
    let msg = recv(state).await?;
    if !msg.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected handshake payload.").into());
    }
    Ok(())
}

/// Using async/await to send a Noise handshake message with an empty payload.
async fn send_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: SplittableReadWrite,
{
    let u = Vec::<u8>::new();
    state.io.send2(&u).await?;
    state.io.flush2().await?;
    info!("send empty payload finished");
    Ok(())
}

/// Using async/await to receive a Noise handshake message with a payload.
async fn recv_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: SplittableReadWrite,
{
    let msg = recv(state).await?;

    let pb = payload_proto::NoiseHandshakePayload::decode(&msg[..])?;

    info!("pb parsed ok");

    if !pb.identity_key.is_empty() {
        let pk = identity::PublicKey::from_protobuf_encoding(&pb.identity_key).map_err(|_| NoiseError::InvalidKey)?;
        if let Some(ref k) = state.id_remote_pubkey {
            if k != &pk {
                return Err(NoiseError::InvalidKey);
            }
        }
        state.id_remote_pubkey = Some(pk);
    }

    if !pb.identity_sig.is_empty() {
        state.dh_remote_pubkey_sig = Some(pb.identity_sig);
    }

    info!("recv identity finished");
    Ok(())
}

/// Send a Noise handshake message with a payload identifying the local node to the remote.
async fn send_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: SplittableReadWrite,
{
    let mut pb = payload_proto::NoiseHandshakePayload::default();
    if state.send_identity {
        pb.identity_key = state.identity.public.clone().into_protobuf_encoding()
    }
    if let Some(ref sig) = state.identity.signature {
        pb.identity_sig = sig.clone()
    }

    let mut msg = Vec::with_capacity(pb.encoded_len());
    pb.encode(&mut msg).expect("Vec<u8> provides capacity as needed");
    state.io.send2(&msg).await?;

    state.io.flush2().await?;
    Ok(())
}
