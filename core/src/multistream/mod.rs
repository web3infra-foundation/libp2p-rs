//! # Multistream-select Protocol Negotiation
//!
//! This crate implements the `multistream-select` protocol, which is the protocol
//! used by libp2p to negotiate which application-layer protocol to use with the
//! remote on a connection or substream.
//!
//! > **Note**: This crate is used primarily by core components of *libp2p* and it
//! > is usually not used directly on its own.
//!
//! ## Roles
//!
//! Two peers using the multistream-select negotiation protocol on an I/O stream
//! are distinguished by their role as a _dialer_ (or _initiator_) or as a _listener_
//! (or _responder_). Thereby the dialer plays the active part, driving the protocol,
//! whereas the listener reacts to the messages received.
//!
//! The dialer has two options: it can either pick a protocol from the complete list
//! of protocols that the listener supports, or it can directly suggest a protocol.
//! Either way, a selected protocol is sent to the listener who can either accept (by
//! echoing the same protocol) or reject (by responding with a message stating
//! "not available"). If a suggested protocol is not available, the dialer may
//! suggest another protocol. This process continues until a protocol is agreed upon,
//! yielding a [`Negotiated`](self::Negotiated) stream, or the dialer has run out of
//! alternatives.
//!
//! See [`dialer_select_proto`](self::dialer_select_proto) and
//! [`listener_select_proto`](self::listener_select_proto).
//!
//! ## [`Negotiated`](self::Negotiated)
//!
//! When a dialer or listener participating in a negotiation settles
//! on a protocol to use, the [`DialerSelectFuture`] respectively
//! [`ListenerSelectFuture`] yields a [`Negotiated`](self::Negotiated)
//! I/O stream.
//!
//! Notably, when a `DialerSelectFuture` resolves to a `Negotiated`, it may not yet
//! have written the last negotiation message to the underlying I/O stream and may
//! still be expecting confirmation for that protocol, despite having settled on
//! a protocol to use.
//!
//! Similarly, when a `ListenerSelectFuture` resolves to a `Negotiated`, it may not
//! yet have sent the last negotiation message despite having settled on a protocol
//! proposed by the dialer that it supports.
//!
//! This behaviour allows both the dialer and the listener to send data
//! relating to the negotiated protocol together with the last negotiation
//! message(s), which, in the case of the dialer only supporting a single
//! protocol, results in 0-RTT negotiation. Note, however, that a dialer
//! that performs multiple 0-RTT negotiations in sequence for different
//! protocols layered on top of each other may trigger undesirable behaviour
//! for a listener not supporting one of the intermediate protocols.
//! See [`dialer_select_proto`](self::dialer_select_proto).

// mod dialer_select;
mod length_delimited;
// mod listener_select;
pub mod muxer;
mod negotiator;
mod protocol;
mod tests;

pub use self::negotiator::{NegotiationError, Negotiator};
pub use self::protocol::{ProtocolError, Version};
// pub use self::dialer_select::{dialer_select_proto, DialerSelectFuture};
// pub use self::listener_select::{listener_select_proto, ListenerSelectFuture};

pub(self) use libp2p_traits::ReadEx;
pub(self) use libp2p_traits::WriteEx;

#[cfg(test)]
pub(self) use tests::Memory;
