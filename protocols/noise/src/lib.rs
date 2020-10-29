// Copyright 2020 Netwarps Ltd.
//

/// `Noise` referred to rust-libp2p and go-libp2p, but use async/await instead of poll.
///
/// Now we just support `xx` pattern.
mod error;
mod io;
mod protocol;

pub use error::NoiseError;
pub use io::handshake;
pub use io::handshake::{IdentityExchange, RemoteIdentity};
pub use io::NoiseOutput;
pub use protocol::{x25519::X25519, x25519_spec::X25519Spec};
pub use protocol::{AuthenticKeypair, Keypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, XX};

use async_trait::async_trait;
use libp2prs_core::identity;
use libp2prs_core::transport::{ConnectionInfo, TransportError};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use libp2prs_traits::SplittableReadWrite;
use zeroize::Zeroize;

/// The protocol upgrade configuration.
#[derive(Clone)]
pub struct NoiseConfig<P, C: Zeroize> {
    dh_keys: AuthenticKeypair<C>,
    local_priv_key: identity::Keypair,
    params: ProtocolParams,
    _marker: std::marker::PhantomData<P>,
}

#[async_trait]
impl<T, C> Upgrader<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: ConnectionInfo + SplittableReadWrite,
    C: Protocol<C> + Zeroize + AsRef<[u8]> + Clone + Send,
{
    type Output = NoiseOutput<T>;

    // server
    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket, false).await
    }

    // client
    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket, true).await
    }
}

async fn make_secure_output<T, C: Protocol<C> + Zeroize + AsRef<[u8]>>(
    config: NoiseConfig<XX, C>,
    socket: T,
    initiator: bool,
) -> Result<NoiseOutput<T>, TransportError>
where
    T: ConnectionInfo + SplittableReadWrite,
{
    let la = socket.local_multiaddr();
    let ra = socket.remote_multiaddr();
    let (_identity, mut output) = config.handshake(socket, initiator).await?;
    output.add_addr(la, ra);
    Ok(output)
}

impl<C> NoiseConfig<XX, C>
where
    C: Protocol<C> + Zeroize + AsRef<[u8]>,
{
    /// Create a new `NoiseConfig` for the `XX` handshake pattern.
    pub fn xx(dh_keys: AuthenticKeypair<C>, local_priv_key: identity::Keypair) -> Self {
        NoiseConfig {
            dh_keys,
            local_priv_key,
            params: C::params_xx(),
            // remote: (),
            _marker: std::marker::PhantomData,
        }
    }

    /// Performs a handshake on the given socket.
    ///
    /// This function use initiator to identify server/client.
    ///
    /// On success, returns an object that implements the `WriteEx` and `ReadEx` trait.
    pub async fn handshake<T>(self, socket: T, initiator: bool) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        T: SplittableReadWrite,
    {
        if initiator {
            let session = self
                .params
                .into_builder()
                .local_private_key(self.dh_keys.secret().as_ref())
                // .remote_public_key(self.remote.0.as_ref())
                .build_initiator()
                .map_err(NoiseError::from);

            handshake::initiator(
                socket,
                session,
                self.dh_keys.into_identity(),
                IdentityExchange::Mutual,
                self.local_priv_key,
            )
            .await
        } else {
            let session = self
                .params
                .into_builder()
                .local_private_key(self.dh_keys.secret().as_ref())
                // .remote_public_key(self.remote.0.as_ref())
                .build_responder()
                .map_err(NoiseError::from);
            handshake::responder(
                socket,
                session,
                self.dh_keys.into_identity(),
                IdentityExchange::Mutual,
                self.local_priv_key,
            )
            .await
        }
    }
}
