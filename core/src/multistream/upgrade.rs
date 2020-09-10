mod security;
mod select;

use std::io;
use std::collections::HashMap;
use async_trait::async_trait;

use super::negotiator::Negotiator;

use std::iter;
use super::{ReadEx, WriteEx};

#[async_trait]
pub trait Upgrader<TSocket> {
    type Output;
    type Proto: Send + AsRef<[u8]> + Clone + Eq + std::hash::Hash;
    type ProtoIter: IntoIterator<Item = Self::Proto>;

    async fn upgrade_inbound(self, socket: TSocket, proto: Self::Proto) -> Self::Output;

    async fn upgrade_outbound(self, socket: TSocket, proto: Self::Proto) -> Self::Output;

    fn protocols(&self) -> Self::ProtoIter;
}

struct UpgradeMuxer;

impl UpgradeMuxer {
    async fn apply_inbound<TSocket, TUpgrader>(socket: TSocket, up: TUpgrader) -> io::Result<TUpgrader::Output>
    where
        TSocket: ReadEx + WriteEx + Send + Sync + Unpin,
        TUpgrader: Upgrader<TSocket>,
    {
        let neg = Negotiator::new_with_protocols(up.protocols());
        let (proto, io) = neg.negotiate(socket).await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(up.upgrade_inbound(io, proto).await)
    }

    async fn apply_outbound<TSocket, TUpgrader>(socket: TSocket, up: TUpgrader) -> io::Result<TUpgrader::Output>
    where
        TSocket: ReadEx + WriteEx + Send + Sync + Unpin,
        TUpgrader: Upgrader<TSocket>,
    {
        let neg = Negotiator::new_with_protocols(up.protocols());
        let (proto, io) = neg.select_one(socket).await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(up.upgrade_outbound(io, proto).await)
    }
}
