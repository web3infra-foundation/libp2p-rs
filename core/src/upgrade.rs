
//! Contains everything related to upgrading a connection or a substream to use a protocol.
//!
//! After a connection with a remote has been successfully established or a substream successfully
//! opened, the next step is to *upgrade* this connection or substream to use a protocol.
//!
//! This is where the `Upgrader` traits come into play.
//! The trait is implemented on types that represent a
//! collection of one or more possible protocols for respectively an ingoing or outgoing
//! connection or substream.
//!
//! > **Note**: Multiple versions of the same protocol are treated as different protocols.
//! >           For example, `/foo/1.0.0` and `/foo/1.1.0` are totally unrelated as far as
//! >           upgrading is concerned.
//!
//! # Upgrade process
//!
//! An upgrade is performed in two steps:
//!
//! - A protocol negotiation step. The `UpgradeInfo::protocol_info` method is called to determine
//!   which protocols are supported by the trait implementation. The `multistream-select` protocol
//!   is used in order to agree on which protocol to use amongst the ones supported.
//!
//! - A handshake. After a successful negotiation, the `Upgrader::upgrade_inbound` or
//!   `Upgrader::upgrade_outbound` method is called. This method will return a upgraded
//!   'Connection'. This handshake is considered mandatory, however in practice it is
//!   possible for the trait implementation to return a dummy `Connection` that doesn't perform any
//!   action and immediately succeeds.
//!
//! After an upgrade is successful, an object of type `Upgrader::Output` is returned.
//! The actual object depends on the implementation and there is no constraint on the traits that
//! it should implement, however it is expected that it can be used by the user to control the
//! behaviour of the protocol.
//!

// mod apply;
// mod denied;
// mod either;
// mod error;
// mod from_fn;
// mod map;
// mod optional;
pub(crate) mod select;
// mod transfer;
pub(crate) mod dummy;
pub(crate) mod multistream;

use async_trait::async_trait;
use crate::transport::TransportError;


pub use self::{
    dummy::DummyUpgrader,
    multistream::Multistream,
    select::Selector,
};


/// Types serving as protocol names.
///
/// # Context
///
/// In situations where we provide a list of protocols that we support,
/// the elements of that list are required to implement the [`ProtocolName`] trait.
///
/// Libp2p will call [`ProtocolName::protocol_name`] on each element of that list, and transmit the
/// returned value on the network. If the remote accepts a given protocol, the element
/// serves as the return value of the function that performed the negotiation.
///
/// # Example
///
/// ```
/// use libp2p_core::ProtocolName;
///
/// enum MyProtocolName {
///     Version1,
///     Version2,
///     Version3,
/// }
///
/// impl ProtocolName for MyProtocolName {
///     fn protocol_name(&self) -> &[u8] {
///         match *self {
///             MyProtocolName::Version1 => b"/myproto/1.0",
///             MyProtocolName::Version2 => b"/myproto/2.0",
///             MyProtocolName::Version3 => b"/myproto/3.0",
///         }
///     }
/// }
/// ```
///
pub trait ProtocolName {
    /// The protocol name as bytes. Transmitted on the network.
    ///
    /// **Note:** Valid protocol names must start with `/` and
    /// not exceed 140 bytes in length.
    fn protocol_name(&self) -> &[u8];
}

impl<T: AsRef<[u8]>> ProtocolName for T {
    fn protocol_name(&self) -> &[u8] {
        self.as_ref()
    }
}

pub trait UpgradeInfo {
    /// Opaque type representing a negotiable protocol.
    type Info: ProtocolName + Clone + Send + std::fmt::Debug;

    /// Returns the list of protocols that are supported. Used during the negotiation process.
    fn protocol_info(&self) -> Vec<Self::Info>;
}

/// Common trait for upgrades that can be applied on inbound substreams, outbound substreams,
/// or both.
/// Possible upgrade on a connection or substream.
#[async_trait]
pub trait Upgrader<C> : UpgradeInfo {
    /// Output after the upgrade has been successfully negotiated and the handshake performed.
    type Output: Send;

    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start the handshake.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    async fn upgrade_inbound(self, socket: C, info: Self::Info) -> Result<Self::Output, TransportError>;

    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start the handshake.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    async fn upgrade_outbound(self, socket: C, info: Self::Info) -> Result<Self::Output, TransportError>;
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_info_multi_versions() {

        #[derive(PartialEq, Debug, Clone)]
        enum MyProtocolName {
            Version1,
            Version2,
            Version3,
        }

        impl ProtocolName for MyProtocolName {
            fn protocol_name(&self) -> &[u8] {
                match *self {
                    MyProtocolName::Version1 => b"/myproto/1.0",
                    MyProtocolName::Version2 => b"/myproto/2.0",
                    MyProtocolName::Version3 => b"/myproto/3.0",
                }
            }
        }

        struct P;

        impl UpgradeInfo for P {
            type Info = MyProtocolName;
            fn protocol_info(&self) -> Vec<Self::Info> {
                vec![
                    MyProtocolName::Version1,
                    MyProtocolName::Version2,
                    MyProtocolName::Version3
                ]
            }
        }

        let p = P {};

        assert_eq!(p.protocol_info().get(0).unwrap(), &MyProtocolName::Version1);
        assert_eq!(p.protocol_info().get(1).unwrap(), &MyProtocolName::Version2);
        assert_eq!(p.protocol_info().get(2).unwrap(), &MyProtocolName::Version3);
    }
}

