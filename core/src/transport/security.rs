
use async_trait::async_trait;
use crate::transport::{Transport, TransportError, TransportListener};
use crate::Multiaddr;
use std::{fmt, io, pin::Pin};
use futures::{prelude::*, task::Context, task::Poll, StreamExt};
use crate::upgrade::Upgrader;

/// Implementation of `SecurityUpgrader` that set up the secured connection
///
/// After a connection has been accepted by the transport, it might be upgraded
/// into a secured connection, f.g., SecIO or TLS

pub struct SecurityUpgrader<T> {
    protos: Vec<T>
}

impl<T> SecurityUpgrader<T>
{
    /// Wraps around a `Upgrader` to select and apply security protocol.
    pub fn new() -> Self {
        SecurityUpgrader { protos: Vec::new() }
    }

    pub fn add(mut self, t: T) -> Self {
        self.protos.push(t);
        self
    }

    pub fn upgrade_outbound<C>(&mut self, c: C)
        where   T: Upgrader<C>,
                C: AsyncRead + AsyncWrite
    {
        let p = self.protos.pop().unwrap();
        p.upgrade_outbound(c);
    }
}

impl<T: Default> Default for SecurityUpgrader<T> {
    fn default() -> Self {
        SecurityUpgrader::new()
    }
}
