// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Connection-oriented communication channels.
//!
//! The main entity of this module is the [`Transport`] trait, which provides an
//! interface for establishing connections with other nodes, thereby negotiating
//! any desired protocols.

use async_trait::async_trait;
use futures::prelude::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{error::Error, fmt};

use libp2prs_multiaddr::Multiaddr;

use crate::either::EitherOutput;
use crate::multistream::NegotiationError;
use crate::pnet::PnetError;
use futures::future::Either;

pub mod dummy;
pub mod memory;
pub mod protector;
pub mod timeout;
pub mod upgrade;

/// A transport provides connection-oriented communication between two peers
/// through ordered streams of data (i.e. connections).
///
/// Connections are established either by [accepting](Transport::IListener::accept)
/// or [dialing](Transport::dial) on a [`Transport`]. A peer that
/// obtains a connection by listening is often referred to as the *listener* and the
/// peer that initiated the connection through dialing as the *dialer*, in
/// contrast to the traditional roles of *server* and *client*.
///
/// Most transports also provide a form of reliable delivery on the established
/// connections but the precise semantics of these guarantees depend on the
/// specific transport.
///
/// This trait is implemented for concrete connection-oriented transport protocols
/// like TCP or Unix Domain Sockets, but also on wrappers that add additional
/// functionality to the dialing or listening process (e.g. name resolution via
/// the DNS).
///

#[async_trait]
pub trait Transport: Send {
    /// The result of a connection setup process, including protocol upgrades.
    ///
    /// Typically the output contains at least a handle to a data stream (i.e. a
    /// connection or a substream multiplexer on top of a connection) that
    /// provides APIs for sending and receiving data through the connection.
    type Output;

    /// Listens on the given [`Multiaddr`], producing a IListener which can be used to accept
    /// new inbound connections.
    ///
    /// Returning an error when there is underlying error in transport.
    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError>;

    /// Dials the given [`Multiaddr`], returning a outbound connection.
    ///
    /// If [`TransportError::MultiaddrNotSupported`] is returned, it means a wrong transport is
    /// used to dial for the address.
    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError>;

    /// Clones the transport and returns the trait object.
    fn box_clone(&self) -> ITransport<Self::Output>;

    /// Returns the [`Multiaddr`] protocol supported by the transport.
    ///
    /// In general, transport supports some concrete protocols, e.g. TCP transport for TCP.
    /// It should always be a match between the transport and the given [`Multiaddr`] to dial/listen.
    /// Otherwise, [`TransportError::MultiaddrNotSupported`] is returned.
    fn protocols(&self) -> Vec<u32>;

    /// Adds a timeout to the connection setup (including upgrades) for all
    /// inbound and outbound connections established through the transport.
    fn timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::new(self, timeout)
    }

    /// Adds a timeout to the connection setup (including upgrades) for all outbound
    /// connections established through the transport.
    fn outbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_outgoing_timeout(self, timeout)
    }

    /// Adds a timeout to the connection setup (including upgrades) for all inbound
    /// connections established through the transport.
    fn inbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_ingoing_timeout(self, timeout)
    }
}

/// Event produced by [`Transport::Listener`]s.
///
/// Transports are expected to produce `Upgrade` events only for
/// listen addresses which have previously been announced via
/// a `NewAddress` event and which have not been invalidated by
/// an `AddressExpired` event yet.
#[derive(Clone, Debug)]
pub enum ListenerEvent<TOutput> {
    /// A new additional [`Multiaddr`] has been added.
    AddressAdded(Multiaddr),
    /// A [`Multiaddr`] is no longer existent.
    AddressDeleted(Multiaddr),
    /// A TOutput has been accepted.
    Accepted(TOutput),
}

impl<TOutput> ListenerEvent<TOutput> {
    /// In case this [`ListenerEvent`] is an Accpeted(), apply the given function
    /// produce another listener event based the the function's result.
    pub fn map<U>(self, f: impl FnOnce(TOutput) -> Result<U, TransportError>) -> Result<ListenerEvent<U>, TransportError> {
        match self {
            ListenerEvent::Accepted(o) => f(o).map(ListenerEvent::Accepted),
            ListenerEvent::AddressAdded(a) => Ok(ListenerEvent::AddressAdded(a)),
            ListenerEvent::AddressDeleted(a) => Ok(ListenerEvent::AddressDeleted(a)),
        }
    }

    /// Returns `true` if this is a `AddressAdded` listener event.
    pub fn is_address_added(&self) -> bool {
        matches!(self, ListenerEvent::AddressAdded(_))
    }

    /// Try to turn this listener event into the `AddressAdded` part.
    ///
    /// Returns `None` if the event is not actually a `AddressAdded`,
    /// otherwise the address.
    pub fn into_new_address(self) -> Option<Multiaddr> {
        if let ListenerEvent::AddressAdded(a) = self {
            Some(a)
        } else {
            None
        }
    }

    /// Returns `true` if this is an `AddressExpired` listener event.
    pub fn is_address_deleted(&self) -> bool {
        matches!(self, ListenerEvent::AddressDeleted(_))
    }

    /// Try to turn this listener event into the `AddressDeleted` part.
    ///
    /// Returns `None` if the event is not actually a `AddressDeleted`,
    /// otherwise the address.
    pub fn into_address_deleted(self) -> Option<Multiaddr> {
        if let ListenerEvent::AddressDeleted(a) = self {
            Some(a)
        } else {
            None
        }
    }
}

#[async_trait]
pub trait TransportListener: Send {
    /// The result of a connection setup process, including protocol upgrades.
    ///
    /// Typically the output contains at least a handle to a data stream (i.e. a
    /// connection or a substream multiplexer on top of a connection) that
    /// provides APIs for sending and receiving data through the connection.
    type Output: Send;

    /// The Listener handles the inbound connections
    async fn accept(&mut self) -> Result<ListenerEvent<Self::Output>, TransportError>;

    /// Returns the local addresses being listened on.
    ///
    /// This might be `None` if it is listening on an unspecified address. The actual
    /// addresses will be reported by ListenerEvent::AddressAdded in this case.
    fn multi_addr(&self) -> Option<&Multiaddr>;

    fn incoming(&mut self) -> Incoming<Self>
    where
        Self: Sized,
    {
        Incoming(self)
    }
    // /// Returns the local network address
    // fn local_addr(&self) -> io::Result<SocketAddr>;

    /// The Listener handles the inbound connections
    async fn accept_output(&mut self) -> Result<Self::Output, TransportError> {
        loop {
            if let ListenerEvent::Accepted(o) = self.accept().await? {
                break Ok(o);
            }
        }
    }
}

/// Trait object for `TransportListener`
pub type IListener<TOutput> = Box<dyn TransportListener<Output = TOutput> + Send>;

#[async_trait]
impl<Output: Send> TransportListener for IListener<Output> {
    type Output = Output;

    async fn accept(&mut self) -> Result<ListenerEvent<Self::Output>, TransportError> {
        (**self).accept().await
    }

    fn multi_addr(&self) -> Option<&Multiaddr> {
        (**self).multi_addr()
    }
}

#[async_trait]
impl<A: TransportListener, B: TransportListener> TransportListener for Either<A, B> {
    type Output = EitherOutput<A::Output, B::Output>;

    async fn accept(&mut self) -> Result<ListenerEvent<Self::Output>, TransportError> {
        match self {
            Either::Left(a) => a.accept().await?.map(|o| Ok(EitherOutput::A(o))),
            Either::Right(b) => b.accept().await?.map(|o| Ok(EitherOutput::B(o))),
        }
    }

    fn multi_addr(&self) -> Option<&Multiaddr> {
        match self {
            Either::Left(a) => a.multi_addr(),
            Either::Right(b) => b.multi_addr(),
        }
    }
}

/// Trait object for `Transport`
pub type ITransport<TOutput> = Box<dyn Transport<Output = TOutput> + Send>;

#[async_trait]
impl<Output: Send> Transport for ITransport<Output> {
    type Output = Output;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        (**self).listen_on(addr)
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        (**self).dial(addr).await
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        (**self).box_clone()
    }

    fn protocols(&self) -> Vec<u32> {
        (**self).protocols()
    }
}

#[async_trait]
impl<A: Transport, B: Transport> Transport for Either<A, B>
where
    A: 'static,
    B: 'static,
    A::Output: Send + 'static,
    B::Output: Send + 'static,
{
    type Output = EitherOutput<A::Output, B::Output>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<IListener<Self::Output>, TransportError> {
        match self {
            Either::Left(a) => a
                .listen_on(addr)
                .map(|l| Box::new(Either::Left::<_, IListener<B::Output>>(l)) as IListener<Self::Output>),
            Either::Right(b) => b
                .listen_on(addr)
                .map(|l| Box::new(Either::Right::<IListener<A::Output>, _>(l)) as IListener<Self::Output>),
        }
    }

    async fn dial(&mut self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        match self {
            Either::Left(a) => a.dial(addr).await.map(EitherOutput::A),
            Either::Right(b) => b.dial(addr).await.map(EitherOutput::B),
        }
    }

    fn box_clone(&self) -> ITransport<Self::Output> {
        match self {
            Either::Left(a) => Box::new(Either::Left::<_, B>(a.box_clone())),
            Either::Right(b) => Box::new(Either::Right::<A, _>(b.box_clone())),
        }
    }

    fn protocols(&self) -> Vec<u32> {
        match self {
            Either::Left(a) => a.protocols(),
            Either::Right(b) => b.protocols(),
        }
    }
}

impl<TOutput: ConnectionInfo> Clone for ITransport<TOutput> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

pub struct Incoming<'a, T>(&'a mut T);

/// Implements Stream for Listener
///
///
impl<'a, T> Stream for Incoming<'a, T>
where
    T: TransportListener,
{
    type Item = Result<ListenerEvent<T::Output>, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let future = self.0.accept();
        futures::pin_mut!(future);

        let evt = futures::ready!(future.poll(cx))?;
        Poll::Ready(Some(Ok(evt)))
    }
}

/// The trait for the connection, which is bound by Transport::Output
/// mark as 'Send' due to Transport::Output must be 'Send'
///
pub trait ConnectionInfo: Send {
    fn local_multiaddr(&self) -> Multiaddr;
    fn remote_multiaddr(&self) -> Multiaddr;
}

/// An error during [dialing][Transport::dial] or [accepting][Transport::IListener::accept]
/// on a [`Transport`].
#[derive(Debug)]
pub enum TransportError {
    /// The [`Multiaddr`] passed as parameter is not supported.
    ///
    /// Contains back the same address.
    MultiaddrNotSupported(Multiaddr),

    /// The connection can not be established in time.
    Timeout,

    /// The memory transport is unreachable.
    Unreachable,

    /// Internal error
    Internal,

    /// Routing error.
    Routing(Box<dyn Error + Send + Sync>),

    /// Any IO error that a [`Transport`] may produce.
    IoError(std::io::Error),

    /// Failed to find any IP address for this DNS address.
    ResolveFail(String),

    /// Multistream selection error.
    NegotiationError(NegotiationError),

    /// Pnet layer error.
    ProtectorError(PnetError),

    /// Security layer error.
    SecurityError(Box<dyn Error + Send + Sync>),

    /// StreamMuxer layer error
    StreamMuxerError(Box<dyn Error + Send + Sync>),

    /// websocket error
    WsError(Box<dyn Error + Send + Sync>),
}

impl From<std::io::Error> for TransportError {
    /// Converts IO error to TransportError
    fn from(e: std::io::Error) -> Self {
        TransportError::IoError(e)
    }
}

impl From<NegotiationError> for TransportError {
    fn from(e: NegotiationError) -> Self {
        TransportError::NegotiationError(e)
    }
}

impl From<PnetError> for TransportError {
    fn from(e: PnetError) -> Self {
        TransportError::ProtectorError(e)
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::MultiaddrNotSupported(addr) => write!(f, "Multiaddr is not supported: {}", addr),
            TransportError::Timeout => write!(f, "Operation timeout"),
            TransportError::Unreachable => write!(f, "Memory transport unreachable"),
            TransportError::Internal => write!(f, "Internal error"),
            TransportError::Routing(err) => write!(f, "Routing layer error {:?}", err),
            TransportError::IoError(err) => write!(f, "IO error {}", err),
            TransportError::ResolveFail(name) => write!(f, "resolve dns {} failed", name),
            TransportError::NegotiationError(err) => write!(f, "Negotiation error {:?}", err),
            TransportError::ProtectorError(err) => write!(f, "Protector error {:?}", err),
            TransportError::SecurityError(err) => write!(f, "SecurityError layer error {:?}", err),
            TransportError::StreamMuxerError(err) => write!(f, "StreamMuxerError layer error {:?}", err),
            TransportError::WsError(err) => write!(f, "Websocket transport  error: {}", err),
        }
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TransportError::MultiaddrNotSupported(_) => None,
            TransportError::Timeout => None,
            TransportError::Unreachable => None,
            TransportError::Internal => None,
            TransportError::Routing(err) => Some(&**err),
            TransportError::IoError(err) => Some(err),
            TransportError::ResolveFail(_) => None,
            TransportError::NegotiationError(err) => Some(err),
            TransportError::ProtectorError(err) => Some(err),
            TransportError::SecurityError(err) => Some(&**err),
            TransportError::StreamMuxerError(err) => Some(&**err),
            TransportError::WsError(err) => Some(&**err),
        }
    }
}
