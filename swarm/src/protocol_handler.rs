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

//! A handler for a set of protocols used on a connection with a remote.
//!
//! This trait should be implemented for a type that maintains the server side state for the
//! execution of a specific protocol.
//!
//! > **Note**:: ProtocolHandler is an async trait and can be made into a trait object.
//!
//! ## UpgradeInfo
//! The trait ProtocolHandler derives from `UpgradeInfo`, which provides a list of protocols
//! that are supported, e.g. '/foo/1.0.0' and '/foo/2.0.0'.
//!
//! ## Handling a protocol
//!
//! Communication with a remote over a set of protocols is initiated in one of two ways:
//!
//! - Dialing by initiating a new outbound substream. In order to do so, `Swarm::control::new_stream()`
//! must be invoked with the specified protocols to create a sub-stream. A protocol negotiation procedure
//! will done for the protocols, in which one might be finally selected. Upon success, a `Swarm::Substream`
//! will be returned by `Swarm::control::new_stream()`, and the protocol will be then handled by the owner
//! of the Substream.
//!
//! - Listening by accepting a new inbound substream. When a new inbound substream is created on a connection,
//! `Swarm::muxer` is called to negotiate the protocol(s). Upon success, `ProtocolHandler::handle` is called
//! with the final output of the upgrade.
//!
//! ## Adding protocol handlers to Swarm
//!
//! In general, multiple protocol handlers should be made into trait objects and then added to `Swarm::muxer`.
//!

use crate::substream::Substream;
use crate::{ProtocolId, SwarmError};
use async_trait::async_trait;
use libp2prs_core::upgrade::{ProtocolName, UpgradeInfo};

/// Common trait for upgrades that can be applied on inbound substreams, outbound substreams,
/// or both.
/// Possible upgrade on a connection or substream.
#[async_trait]
pub trait ProtocolHandler: UpgradeInfo {
    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start handling the inbound. Swarm will start invoking this method
    /// in a newly spawned task.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    async fn handle(&mut self, stream: Substream, info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError>;
    /// This is to provide a clone method for the trait object.
    fn box_clone(&self) -> IProtocolHandler;
}

pub type IProtocolHandler = Box<dyn ProtocolHandler<Info = ProtocolId> + Send + Sync>;

impl Clone for IProtocolHandler {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Dummy protocol handler, test purpose
///
/// Implementation of `ProtocolHandler` that doesn't handle anything.
#[derive(Clone, Default)]
pub struct DummyProtocolHandler {}

impl DummyProtocolHandler {
    pub fn new() -> Self {
        DummyProtocolHandler {}
    }
}

impl UpgradeInfo for DummyProtocolHandler {
    type Info = &'static [u8];
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/dummy/1.0.0", b"/dummy/2.0.0"]
    }
}

#[async_trait]
impl ProtocolHandler for DummyProtocolHandler {
    async fn handle(&mut self, stream: Substream, info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError> {
        log::trace!("Dummy Protocol handling inbound {:?} {:?}", stream, info.protocol_name_str());
        Ok(())
    }
    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}

/*

/// Configuration of inbound or outbound substream protocol(s)
/// for a [`ProtocolsHandler`].
///
/// The inbound substream protocol(s) are defined by [`ProtocolsHandler::listen_protocol`]
/// and the outbound substream protocol(s) by [`ProtocolsHandlerEvent::OutboundSubstreamRequest`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SubstreamProtocol<TUpgrade> {
    upgrade: TUpgrade,
    timeout: Duration,
}

impl<TUpgrade> SubstreamProtocol<TUpgrade> {
    /// Create a new `SubstreamProtocol` from the given upgrade.
    ///
    /// The default timeout for applying the given upgrade on a substream is
    /// 10 seconds.
    pub fn new(upgrade: TUpgrade) -> SubstreamProtocol<TUpgrade> {
        SubstreamProtocol {
            upgrade,
            timeout: Duration::from_secs(10),
        }
    }

    /// Sets the multistream-select protocol (version) to use for negotiating
    /// protocols upgrades on outbound substreams.
    pub fn with_upgrade_protocol(mut self, version: upgrade::Version) -> Self {
        self.upgrade_protocol = version;
        self
    }

    /// Maps a function over the protocol upgrade.
    pub fn map_upgrade<U, F>(self, f: F) -> SubstreamProtocol<U>
    where
        F: FnOnce(TUpgrade) -> U,
    {
        SubstreamProtocol {
            upgrade: f(self.upgrade),
            timeout: self.timeout,
        }
    }

    /// Sets a new timeout for the protocol upgrade.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Borrows the contained protocol upgrade.
    pub fn upgrade(&self) -> &TUpgrade {
        &self.upgrade
    }

    /// Borrows the timeout for the protocol upgrade.
    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    /// Converts the substream protocol configuration into the contained upgrade.
    pub fn into_upgrade(self) -> (upgrade::Version, TUpgrade) {
        (self.upgrade_protocol, self.upgrade)
    }
}

impl<TUpgrade> From<TUpgrade> for SubstreamProtocol<TUpgrade> {
    fn from(upgrade: TUpgrade) -> SubstreamProtocol<TUpgrade> {
        SubstreamProtocol::new(upgrade)
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr> {
    /// Request a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest {
        /// The protocol(s) to apply on the substream.
        protocol: SubstreamProtocol<TConnectionUpgrade>,
        /// User-defined information, passed back when the substream is open.
        info: TOutboundOpenInfo,
    },

    /// Close the connection for the given reason.
    Close(TErr),

    /// Other event.
    Custom(TCustom),
}

/// Event produced by a handler.
impl<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr>
    ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr>
{
    /// If this is an `OutboundSubstreamRequest`, maps the `info` member from a
    /// `TOutboundOpenInfo` to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, I, TCustom, TErr>
    where
        F: FnOnce(TOutboundOpenInfo) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol,
                    info: map(info),
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
            ProtocolsHandlerEvent::Close(val) => ProtocolsHandlerEvent::Close(val),
        }
    }

    /// If this is an `OutboundSubstreamRequest`, maps the protocol (`TConnectionUpgrade`)
    /// to something else.
    #[inline]
    pub fn map_protocol<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<I, TOutboundOpenInfo, TCustom, TErr>
    where
        F: FnOnce(TConnectionUpgrade) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_upgrade(map),
                    info,
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
            ProtocolsHandlerEvent::Close(val) => ProtocolsHandlerEvent::Close(val),
        }
    }

    /// If this is a `Custom` event, maps the content to something else.
    #[inline]
    pub fn map_custom<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, I, TErr>
    where
        F: FnOnce(TCustom) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(map(val)),
            ProtocolsHandlerEvent::Close(val) => ProtocolsHandlerEvent::Close(val),
        }
    }

    /// If this is a `Close` event, maps the content to something else.
    #[inline]
    pub fn map_close<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, I>
    where
        F: FnOnce(TErr) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
            ProtocolsHandlerEvent::Close(val) => ProtocolsHandlerEvent::Close(map(val)),
        }
    }
}

/// Error that can happen on an outbound substream opening attempt.
#[derive(Debug)]
pub enum ProtocolsHandlerUpgrErr<TUpgrErr> {
    /// The opening attempt timed out before the negotiation was fully completed.
    Timeout,
    /// There was an error in the timer used.
    Timer,
    /// Error while upgrading the substream to the protocol we want.
    Upgrade(UpgradeError<TUpgrErr>),
}

impl<TUpgrErr> fmt::Display for ProtocolsHandlerUpgrErr<TUpgrErr>
where
    TUpgrErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolsHandlerUpgrErr::Timeout => {
                write!(f, "Timeout error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::Timer => {
                write!(f, "Timer error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::Upgrade(err) => write!(f, "{}", err),
        }
    }
}

impl<TUpgrErr> error::Error for ProtocolsHandlerUpgrErr<TUpgrErr>
where
    TUpgrErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            ProtocolsHandlerUpgrErr::Timeout => None,
            ProtocolsHandlerUpgrErr::Timer => None,
            ProtocolsHandlerUpgrErr::Upgrade(err) => Some(err),
        }
    }
}

/// How long the connection should be kept alive.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeepAlive {
    /// If nothing new happens, the connection should be closed at the given `Instant`.
    Until(Instant),
    /// Keep the connection alive.
    Yes,
    /// Close the connection as soon as possible.
    No,
}

impl KeepAlive {
    /// Returns true for `Yes`, false otherwise.
    pub fn is_yes(&self) -> bool {
        match *self {
            KeepAlive::Yes => true,
            _ => false,
        }
    }
}

impl PartialOrd for KeepAlive {
    fn partial_cmp(&self, other: &KeepAlive) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KeepAlive {
    fn cmp(&self, other: &KeepAlive) -> Ordering {
        use self::KeepAlive::*;

        match (self, other) {
            (No, No) | (Yes, Yes)  => Ordering::Equal,
            (No,  _) | (_,   Yes)  => Ordering::Less,
            (_,  No) | (Yes,   _)  => Ordering::Greater,
            (Until(t1), Until(t2)) => t1.cmp(t2),
        }
    }
}
*/
