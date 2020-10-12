mod error;
mod io;
mod protocol;

pub use error::NoiseError;
pub use io::handshake;
pub use io::handshake::{IdentityExchange, RemoteIdentity};
pub use io::NoiseOutput;
pub use protocol::{x25519::X25519, x25519_spec::X25519Spec};
pub use protocol::{AuthenticKeypair, Keypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, IK, IX, XX};

use async_trait::async_trait;
use libp2p_core::identity;
use libp2p_core::transport::{ConnectionInfo, TransportError};
use libp2p_core::upgrade::{UpgradeInfo, Upgrader};
use libp2p_traits::{ReadEx, WriteEx};
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
    T: ConnectionInfo + ReadEx + WriteEx + Send + Unpin + 'static,
    C: Protocol<C> + Zeroize + AsRef<[u8]> + Clone + Send,
{
    type Output = NoiseOutput<T>;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        make_secure_output(self, socket, false).await
    }

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
    T: ConnectionInfo + ReadEx + WriteEx + Send + Unpin + 'static,
{
    let la = socket.local_multiaddr();
    let ra = socket.remote_multiaddr();
    let (_identity, mut output) = config.handshake(socket, initiator).await?;
    output.add_addr(la, ra);
    Ok(output)
}

// impl<C> NoiseConfig<IX, C>
//     where
//         C: Protocol<C> + Zeroize + AsRef<[u8]>,
// {
//     /// Create a new `NoiseConfig` for the `IX` handshake pattern.
//     pub fn ix(keypair: identity::Keypair) -> Self {
//         let dh_keys = Keypair::from_identity(&keypair).unwrap();
//         NoiseConfig {
//             dh_keys,
//             local_priv_key: keypair,
//             params: C::params_ix(),
//             remote: (),
//             _marker: std::marker::PhantomData,
//         }
//     }
//
//     pub async fn handshake<T>(
//         self,
//         socket: T,
//         initiator: bool,
//     ) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
//         where
//             T: ReadEx + WriteEx + Unpin + Send + 'static,
//     {
//         let session = self
//             .params
//             .into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         if initiator {
//             handshake::rt1_initiator(
//                 socket,
//                 session,
//                 self.dh_keys.into_identity(),
//                 IdentityExchange::Mutual,
//                 self.local_priv_key,
//             )
//                 .await
//         } else {
//             handshake::rt1_responder(
//                 socket,
//                 session,
//                 self.dh_keys.into_identity(),
//                 IdentityExchange::Mutual,
//                 self.local_priv_key,
//             )
//                 .await
//         }
//     }
// }

impl<C> NoiseConfig<XX, C>
where
    C: Protocol<C> + Zeroize + AsRef<[u8]>,
{
    /// Create a new `NoiseConfig` for the `XX` handshake pattern.
    // pub fn xx(dh_keys: AuthenticKeypair<C>) -> Self {
    pub fn xx(dh_keys: AuthenticKeypair<C>, local_priv_key: identity::Keypair) -> Self {
        NoiseConfig {
            dh_keys,
            local_priv_key,
            params: C::params_xx(),
            // remote: (),
            _marker: std::marker::PhantomData,
        }
    }

    pub async fn handshake<T>(self, socket: T, initiator: bool) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        T: ReadEx + WriteEx + Unpin + Send + 'static,
    {
        if initiator {
            let session = self
                .params
                .into_builder()
                .local_private_key(self.dh_keys.secret().as_ref())
                // .remote_public_key(self.remote.0.as_ref())
                .build_initiator()
                .map_err(NoiseError::from);

            handshake::rt15_initiator(
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
            handshake::rt15_responder(
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

// impl<C> NoiseConfig<IK, C>
//     where
//         C: Protocol<C> + Zeroize + AsRef<[u8]>,
// {
//     /// Create a new `NoiseConfig` for the `IK` handshake pattern (recipient side).
//     ///
//     /// Since the identity of the local node is known to the remote, this configuration
//     /// does not transmit a static DH public key or public identity key to the remote.
//     pub fn ik_listener(local_priv_key: identity::Keypair) -> Self {
//         let dh_keys = Keypair::from_identity(&local_priv_key).unwrap();
//         NoiseConfig {
//             dh_keys,
//             local_priv_key,
//             params: C::params_ik(),
//             remote: (),
//             _marker: std::marker::PhantomData,
//         }
//     }
//
//     pub async fn handshake<T>(
//         self,
//         socket: T,
//     ) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
//         where
//             T: ReadEx + WriteEx + Unpin + Send + 'static,
//     {
//         let session = self
//             .params
//             .into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         handshake::rt15_responder(
//             socket,
//             session,
//             self.dh_keys.into_identity(),
//             IdentityExchange::Mutual,
//             self.local_priv_key,
//         )
//             .await
//     }
// }
//
// impl<C> NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
//     where
//         C: Protocol<C> + Zeroize + AsRef<[u8]>,
// {
//     /// Create a new `NoiseConfig` for the `IK` handshake pattern (initiator side).
//     ///
//     /// In this configuration, the remote identity is known to the local node,
//     /// but the local node still needs to transmit its own public identity.
//     pub fn ik_dialer(
//         local_priv_key: identity::Keypair,
//         remote_id: identity::PublicKey,
//         remote_dh: PublicKey<C>,
//     ) -> Self {
//         let dh_keys = Keypair::from_identity(&local_priv_key).unwrap();
//         NoiseConfig {
//             dh_keys,
//             local_priv_key,
//             params: C::params_ik(),
//             remote: (remote_dh, remote_id),
//             _marker: std::marker::PhantomData,
//         }
//     }
//
//     pub async fn handshake<T>(
//         self,
//         socket: T,
//     ) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
//         where
//             T: ReadEx + WriteEx + Unpin + Send + 'static,
//     {
//         let session = self
//             .params
//             .into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .remote_public_key(self.remote.0.as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         handshake::rt15_initiator(
//             socket,
//             session,
//             self.dh_keys.into_identity(),
//             IdentityExchange::Mutual,
//             self.local_priv_key,
//         )
//             .await
//     }
// }

// Handshake pattern IX /////////////////////////////////////////////////////

// impl<T, C> NoiseConfig<IX, C>
//     where
//         NoiseConfig<IX, C>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_inbound(self, socket: T, _: <NoiseConfig<protocol::IX, C> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_responder()
//             .map_err(NoiseError::from);
//         handshake::rt1_responder(socket, session,
//                                  self.dh_keys.into_identity(),
//                                  IdentityExchange::Mutual)
//     }
// }
//
// impl<T, C> NoiseConfig<IX, C>
//     where
//         NoiseConfig<IX, C>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_outbound(self, socket: T, _: <NoiseConfig<protocol::IX, C> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         handshake::rt1_initiator(socket, session,
//                                  self.dh_keys.into_identity(),
//                                  IdentityExchange::Mutual)
//     }
// }
//
// // Handshake pattern XX /////////////////////////////////////////////////////
//
// impl<T, C> NoiseConfig<XX, C>
//     where
//         NoiseConfig<XX, C>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_inbound(self, socket: T, _: <NoiseConfig<protocol::XX, C> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_responder()
//             .map_err(NoiseError::from);
//         handshake::rt15_responder(socket, session,
//                                   self.dh_keys.into_identity(),
//                                   IdentityExchange::Mutual)
//     }
// }
//
// impl<T, C> NoiseConfig<XX, C>
//     where
//         NoiseConfig<XX, C>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_outbound(self, socket: T, _: <NoiseConfig<protocol::XX, C> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         handshake::rt15_initiator(socket, session,
//                                   self.dh_keys.into_identity(),
//                                   IdentityExchange::Mutual)
//     }
// }
//
// // Handshake pattern IK /////////////////////////////////////////////////////
//
// impl<T, C, R> NoiseConfig<IK, C, R>
//     where
//         NoiseConfig<IK, C, R>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_inbound(self, socket: T, _: <NoiseConfig<protocol::IK, C, R> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .build_responder()
//             .map_err(NoiseError::from);
//         handshake::rt1_responder(socket, session,
//                                  self.dh_keys.into_identity(),
//                                  IdentityExchange::Receive)
//     }
// }
//
// impl<T, C> NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
//     where
//         NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>: UpgradeInfo,
//         T: ReadEx + WriteEx + Unpin + Send + 'static,
//         C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     // type Output = (RemoteIdentity<C>, NoiseOutput<T>);
//     // type Error = NoiseError;
//     // type Future = Handshake<T, C>;
//
//     fn upgrade_outbound(self, socket: T, _: <NoiseConfig<protocol::IK, C, (protocol::PublicKey<C>, libp2p_core::PublicKey)> as Trait>::Info) -> Handshake<T, C> {
//         let session = self.params.into_builder()
//             .local_private_key(self.dh_keys.secret().as_ref())
//             .remote_public_key(self.remote.0.as_ref())
//             .build_initiator()
//             .map_err(NoiseError::from);
//         handshake::rt1_initiator(socket, session,
//                                  self.dh_keys.into_identity(),
//                                  IdentityExchange::Send { remote: self.remote.1 })
//     }
// }

// Authenticated Upgrades /////////////////////////////////////////////////////

// A `NoiseAuthenticated` transport upgrade that wraps around any
// `NoiseConfig` handshake and verifies that the remote identified with a
// [`RemoteIdentity::IdentityKey`], aborting otherwise.
//
// See [`NoiseConfig::into_authenticated`].
//
// On success, the upgrade yields the [`PeerId`] obtained from the
// `RemoteIdentity`. The output of this upgrade is thus directly suitable
// for creating an [`authenticated`](libp2p_core::transport::upgrade::Authenticate)
// transport for use with a [`Network`](libp2p_core::Network).
// #[derive(Clone)]
// pub struct NoiseAuthenticated<P, C: Zeroize, R> {
//     config: NoiseConfig<P, C, R>
// }
//
// impl<P, C: Zeroize, R> UpgradeInfo for NoiseAuthenticated<P, C, R>
// where
//     NoiseConfig<P, C, R>: UpgradeInfo
// {
//     type Info = <NoiseConfig<P, C, R> as UpgradeInfo>::Info;
//     // type InfoIter = <NoiseConfig<P, C, R> as UpgradeInfo>::InfoIter;
//
//     fn protocol_info(&self) -> Vec<Self::Info> {
//         self.config.protocol_info()
//     }
// }
//
// impl<T, P, C, R> InboundUpgrade<T> for NoiseAuthenticated<P, C, R>
// where
//     NoiseConfig<P, C, R>: UpgradeInfo + InboundUpgrade<T,
//         Output = (RemoteIdentity<C>, NoiseOutput<T>),
//         Error = NoiseError
//     > + 'static,
//     <NoiseConfig<P, C, R> as InboundUpgrade<T>>::Future: Send,
//     T: AsyncRead + AsyncWrite + Send + 'static,
//     C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     type Output = (PeerId, NoiseOutput<T>);
//     type Error = NoiseError;
//     type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
//
//     fn upgrade_inbound(self, socket: T, info: Self::Info) -> Self::Future {
//         Box::pin(self.config.upgrade_inbound(socket, info)
//             .and_then(|(remote, io)| match remote {
//                 RemoteIdentity::IdentityKey(pk) => future::ok((pk.into_peer_id(), io)),
//                 _ => future::err(NoiseError::AuthenticationFailed)
//             }))
//     }
// }
//
// impl<T, P, C, R> OutboundUpgrade<T> for NoiseAuthenticated<P, C, R>
// where
//     NoiseConfig<P, C, R>: UpgradeInfo + OutboundUpgrade<T,
//         Output = (RemoteIdentity<C>, NoiseOutput<T>),
//         Error = NoiseError
//     > + 'static,
//     <NoiseConfig<P, C, R> as OutboundUpgrade<T>>::Future: Send,
//     T: AsyncRead + AsyncWrite + Send + 'static,
//     C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
// {
//     type Output = (PeerId, NoiseOutput<T>);
//     type Error = NoiseError;
//     type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
//
//     fn upgrade_outbound(self, socket: T, info: Self::Info) -> Self::Future {
//         Box::pin(self.config.upgrade_outbound(socket, info)
//             .and_then(|(remote, io)| match remote {
//                 RemoteIdentity::IdentityKey(pk) => future::ok((pk.into_peer_id(), io)),
//                 _ => future::err(NoiseError::AuthenticationFailed)
//             }))
//     }
// }
