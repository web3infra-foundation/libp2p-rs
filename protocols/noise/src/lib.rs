// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! [Noise protocol framework][noise] support for libp2p.
//!
//! > **Note**: This crate is still experimental and subject to major breaking changes
//! >           both on the API and the wire protocol.
//!
//! This crate provides `libp2p_core::InboundUpgrade` and `libp2p_core::OutboundUpgrade`
//! implementations for various noise handshake patterns (currently `IK`, `IX`, and `XX`)
//! over a particular choice of Diffie–Hellman key agreement (currently only X25519).
//!
//! > **Note**: Only the `XX` handshake pattern is currently guaranteed to provide
//! >           interoperability with other libp2p implementations.
//!
//! All upgrades produce as output a pair, consisting of the remote's static public key
//! and a `NoiseOutput` which represents the established cryptographic session with the
//! remote, implementing `futures::io::AsyncRead` and `futures::io::AsyncWrite`.
//!
//! [noise]: http://noiseprotocol.org/

mod error;
mod io;
mod protocol;
mod upgrade;

pub use error::NoiseError;
pub use io::handshake;
pub use io::handshake::{Handshake, IdentityExchange, RemoteIdentity};
pub use io::NoiseOutput;
pub use protocol::{x25519::X25519, x25519_spec::X25519Spec};
pub use protocol::{AuthenticKeypair, Keypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, IK, IX, XX};

use futures::prelude::*;
use libp2prs_core::identity;
use zeroize::Zeroize;

/// The protocol upgrade configuration.
#[derive(Clone)]
pub struct NoiseConfig<P, C: Zeroize, R = ()> {
    dh_keys: AuthenticKeypair<C>,
    local_priv_key: identity::Keypair,
    params: ProtocolParams,
    legacy: LegacyConfig,
    _remote: R,
    _marker: std::marker::PhantomData<P>,
}

impl<H, C: Zeroize, R> NoiseConfig<H, C, R> {
    /// Sets the legacy configuration options to use, if any.
    pub fn set_legacy_config(&mut self, cfg: LegacyConfig) -> &mut Self {
        self.legacy = cfg;
        self
    }
}

impl<C> NoiseConfig<XX, C>
where
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    /// Create a new `NoiseConfig` for the `XX` handshake pattern.
    pub fn xx(dh_keys: AuthenticKeypair<C>, local_priv_key: identity::Keypair) -> Self {
        NoiseConfig {
            dh_keys,
            local_priv_key,
            params: C::params_xx(),
            legacy: LegacyConfig::default(),
            _remote: (),
            _marker: std::marker::PhantomData,
        }
    }

    pub async fn handshake<T>(self, socket: T, initiator: bool) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let builder = self.params.into_builder().local_private_key(self.dh_keys.secret().as_ref());

        let session = {
            if initiator {
                builder.build_initiator()
            } else {
                builder.build_responder()
            }
            .map_err(NoiseError::from)
        };

        if initiator {
            handshake::rt15_initiator::<T, C>(socket, session, self.dh_keys.into_identity(), IdentityExchange::Mutual, self.legacy)
        } else {
            handshake::rt15_responder::<T, C>(socket, session, self.dh_keys.into_identity(), IdentityExchange::Mutual, self.legacy)
        }
        .await
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////

/*
impl<T, C> InboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        handshake::rt15_responder(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual,
            self.legacy)
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        handshake::rt15_initiator(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual,
            self.legacy)
    }
}
 */

/// Legacy configuration options.
#[derive(Clone, Default)]
pub struct LegacyConfig {
    /// Whether to continue sending legacy handshake payloads,
    /// i.e. length-prefixed protobuf payloads inside a length-prefixed
    /// noise frame. These payloads are not interoperable with other
    /// libp2p implementations.
    pub send_legacy_handshake: bool,
    /// Whether to support receiving legacy handshake payloads,
    /// i.e. length-prefixed protobuf payloads inside a length-prefixed
    /// noise frame. These payloads are not interoperable with other
    /// libp2p implementations.
    pub recv_legacy_handshake: bool,
}
