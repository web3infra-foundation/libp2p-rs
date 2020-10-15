use std::convert::TryFrom as _;
use std::{fmt, io};
// use std::sync::atomic::AtomicUsize;
// use futures::stream::{Stream, TryStreamExt};
// use std::sync::Arc;
// use async_std::sync::Mutex;

use super::{
    protocol::{Message, MessageIO, Protocol, ProtocolError, Version},
    ReadEx, WriteEx,
};

pub struct Negotiator<TProto> {
    protocols: Vec<(TProto, Protocol)>,
}

impl<TProto: AsRef<[u8]> + Clone> Negotiator<TProto> {
    pub fn new() -> Self {
        Negotiator { protocols: Vec::new() }
    }

    pub fn new_with_protocols<Iter>(protocols: Iter) -> Self
    where
        Iter: IntoIterator<Item = TProto>,
    {
        let protocols: Vec<_> = protocols
            .into_iter()
            .filter_map(|n| match Protocol::try_from(n.as_ref()) {
                Ok(p) => Some((n, p)),
                Err(e) => {
                    log::warn!(
                        "Listener: Ignoring invalid protocol: {} due to {}",
                        String::from_utf8_lossy(n.as_ref()),
                        e
                    );
                    None
                }
            })
            .collect();

        Negotiator { protocols }
    }

    pub fn add_protocol(&mut self, proto: TProto) -> Result<(), ProtocolError> {
        let proto = Protocol::try_from(proto.as_ref()).map(|p| (proto, p))?;
        self.protocols.push(proto);
        Ok(())
    }

    pub async fn negotiate<TSocket>(&self, socket: TSocket) -> Result<(TProto, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Unpin,
    {
        let mut io = MessageIO::new(socket);
        let msg = io.recv_message().await?;
        let version = if let Message::Header(v) = msg {
            v
        } else {
            return Err(ProtocolError::InvalidMessage.into());
        };

        io.send_message(Message::Header(version)).await?;

        loop {
            let msg = io.recv_message().await?;

            match msg {
                Message::ListProtocols => {
                    let supported = self.protocols.iter().map(|(_, p)| p).cloned().collect();
                    let message = Message::Protocols(supported);
                    io.send_message(message).await?;
                }
                Message::Protocol(p) => {
                    let protocol = self
                        .protocols
                        .iter()
                        .find_map(|(name, proto)| if &p == proto { Some(name.clone()) } else { None });

                    let message = if protocol.is_some() {
                        log::debug!("Listener: confirming protocol: {}", p);
                        Message::Protocol(p.clone())
                    } else {
                        log::debug!("Listener: rejecting protocol: {}", String::from_utf8_lossy(p.as_ref()));
                        Message::NotAvailable
                    };

                    io.send_message(message).await?;

                    if let Some(protocol) = protocol {
                        log::debug!("Listener: sent confirmed protocol: {}", String::from_utf8_lossy(protocol.as_ref()));
                        let io = io.into_inner();
                        return Ok((protocol, io));
                    }
                }
                _ => return Err(ProtocolError::InvalidMessage.into()),
            }
        }
    }

    pub async fn select_one<TSocket>(&self, socket: TSocket) -> Result<(TProto, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Unpin,
    {
        let mut io = MessageIO::new(socket);

        let version = Version::default();
        io.send_message(Message::Header(version)).await?;

        let msg = io.recv_message().await?;
        if msg != Message::Header(version) {
            return Err(ProtocolError::InvalidMessage.into());
        }

        for proto in &self.protocols {
            io.send_message(Message::Protocol(proto.1.clone())).await?;
            log::debug!("Dialer: Proposed protocol: {}", proto.1);
            let msg = io.recv_message().await?;

            match msg {
                Message::Protocol(ref p) if p.as_ref() == proto.0.as_ref() => {
                    log::debug!("Dialer: Received confirmation for protocol: {}", p);
                    let io = io.into_inner();
                    return Ok((proto.0.clone(), io));
                }
                Message::NotAvailable => {
                    log::debug!(
                        "Dialer: Received rejection of protocol: {}",
                        String::from_utf8_lossy(proto.0.as_ref())
                    );
                    continue;
                }
                _ => return Err(ProtocolError::InvalidMessage.into()),
            }
        }
        Err(NegotiationError::Failed)
    }
}

impl<TProto: AsRef<[u8]> + Clone> Default for Negotiator<TProto> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum NegotiationError {
    /// A protocol error occurred during the negotiation.
    ProtocolError(ProtocolError),

    /// Protocol negotiation failed because no protocol could be agreed upon.
    Failed,
}

impl From<ProtocolError> for NegotiationError {
    fn from(err: ProtocolError) -> NegotiationError {
        NegotiationError::ProtocolError(err)
    }
}

impl From<io::Error> for NegotiationError {
    fn from(err: io::Error) -> NegotiationError {
        ProtocolError::from(err).into()
    }
}

impl From<NegotiationError> for io::Error {
    fn from(err: NegotiationError) -> io::Error {
        if let NegotiationError::ProtocolError(e) = err {
            return e.into();
        }
        io::Error::new(io::ErrorKind::Other, err)
    }
}

impl std::error::Error for NegotiationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NegotiationError::ProtocolError(err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            NegotiationError::ProtocolError(p) => fmt.write_fmt(format_args!("Protocol error: {}", p)),
            NegotiationError::Failed => fmt.write_str("Protocol negotiation failed."),
        }
    }
}
