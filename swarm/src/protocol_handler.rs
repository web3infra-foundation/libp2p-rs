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

use async_trait::async_trait;
use libp2prs_core::upgrade::UpgradeInfo;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_runtime::task;
use std::error::Error;

use crate::connection::Connection;
use crate::substream::Substream;
use crate::{Control, ProtocolId};

/// Notifiee is an trait for an object wishing to receive notifications from swarm.
pub trait Notifiee {
    /// It is emitted when a connection is connected.
    fn connected(&mut self, _conn: &mut Connection) {}
    /// It is emitted when a connection is disconnected.
    fn disconnected(&mut self, _conn: &mut Connection) {}
    /// It is emitted when finishing identified a remote peer. Therefore,
    /// the multiaddr and protocols of the remote peer can be retrieved
    /// from the PeerStore.
    fn identified(&mut self, _peer: PeerId) {}
    /// It is emitted when the listen addresses for the local host changes.
    /// This might happen for some reasons, f.g., interface up/down.
    ///
    /// The notification contains a snapshot of the current listen addresses.
    fn address_changed(&mut self, _addrs: Vec<Multiaddr>) {}
}

/// Common trait for describing a Swarm friendly protocol.
pub trait ProtocolImpl {
    /// Returns the trait object of the ProtocolHandler, which can be used by Swarm
    /// to construct the protocol muxer.
    fn handler(&self) -> IProtocolHandler;
    /// start() will consume the ownership and start the protocol. An optional
    /// task handle might be returned by start(), and it could be used to track
    /// the lifetime of the protocol.
    fn start(self, _swarm: Control) -> Option<task::TaskHandle<()>>
    where
        Self: Sized,
    {
        None
    }
}

/// Common trait for handling protocol inbound sub-stream.
#[async_trait]
pub trait ProtocolHandler: UpgradeInfo + Notifiee {
    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start handling the inbound. Swarm will start invoking this method
    /// in a newly spawned runtime.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    async fn handle(&mut self, stream: Substream, info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>>;
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
pub struct DummyProtocol;

#[allow(clippy::new_without_default)]
impl DummyProtocol {
    pub fn new() -> Self {
        DummyProtocol
    }
}

impl ProtocolImpl for DummyProtocol {
    fn handler(&self) -> IProtocolHandler {
        Box::new(DummyProtocolHandler)
    }
}

#[derive(Clone)]
struct DummyProtocolHandler;

impl UpgradeInfo for DummyProtocolHandler {
    type Info = ProtocolId;
    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![
            ProtocolId::from(b"/dummy/1.0.0" as &[u8]),
            ProtocolId::from(b"/dummy/2.0.0" as &[u8]),
        ]
    }
}

impl Notifiee for DummyProtocolHandler {}

#[async_trait]
impl ProtocolHandler for DummyProtocolHandler {
    async fn handle(&mut self, stream: Substream, info: <Self as UpgradeInfo>::Info) -> Result<(), Box<dyn Error>> {
        log::trace!("Dummy Protocol handling inbound {:?} {:?}", stream, info);
        Ok(())
    }
    fn box_clone(&self) -> IProtocolHandler {
        Box::new(self.clone())
    }
}
