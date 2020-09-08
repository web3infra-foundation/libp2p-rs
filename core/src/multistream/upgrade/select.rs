use std::iter;
use async_trait::async_trait;

use super::Upgrader;

#[derive(Debug, Clone)]
pub struct SelectUpgrade<A, B>(A, B);

impl<A, B> SelectUpgrade<A, B> {
    /// Combines two upgrades into an `SelectUpgrade`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        SelectUpgrade(a, b)
    }
}

#[async_trait]
impl<TSocket: Send + 'static, A, B> Upgrader<TSocket> for SelectUpgrade<A, B>
where
    A: Upgrader<TSocket>,
    B: Upgrader<TSocket, Proto = A::Proto, ProtoIter = A::ProtoIter>,
    A: Send,
    B: Send,
{
    type Output = SelectOutput<A::Output, B::Output>;
    type Proto = A::Proto;
    type ProtoIter = iter::Chain<<A::ProtoIter as IntoIterator>::IntoIter, <B::ProtoIter as IntoIterator>::IntoIter>;

    async fn upgrade_inbound(self, socket: TSocket, proto: Self::Proto) -> Self::Output {
        let matched = self.0.protocols().into_iter()
            .any(|e| e == proto);
        if matched {
            return SelectOutput::First(self.0.upgrade_inbound(socket, proto).await);
        }
        let matched = self.1.protocols().into_iter().any(|e| e == proto);
        if matched {
            return SelectOutput::Second(self.1.upgrade_inbound(socket, proto).await);
        }
        unreachable!()
    }

    async fn upgrade_outbound(self, socket: TSocket, proto: Self::Proto) -> Self::Output {
        let matched = self.0.protocols().into_iter().any(|e| e == proto);
        if matched {
            return SelectOutput::First(self.0.upgrade_inbound(socket, proto).await);
        }
        let matched = self.1.protocols().into_iter().any(|e| e == proto);
        if matched {
            return SelectOutput::Second(self.1.upgrade_inbound(socket, proto).await);
        }
        unreachable!()
    }

    fn protocols(&self) -> Self::ProtoIter {
        self.0.protocols().into_iter().chain(self.1.protocols())
    }
}

#[derive(Debug, Copy, Clone)]
pub enum SelectOutput<A, B> {
    First(A),
    Second(B),
}

#[cfg(test)]
mod tests {
    use async_std::task;

    use super::{SelectUpgrade, SelectOutput};
    use super::super::{
        UpgradeMuxer,
        super::Memory,
        security::{
            secio::{SecioUpgrade, SecioOutput},
            tls::{TLSUpgrade, TLSOutput},
        }
    };
    use bytes::Bytes;

    #[test]
    fn test_select() {

        let (client_conn, server_conn) = Memory::pair();

        let server_muxer = {
            let secio = SecioUpgrade::new();
            let tls = TLSUpgrade::new();
            let select = SelectUpgrade::new(secio, tls);
            UpgradeMuxer::apply_inbound(server_conn, select)
        };

        let client_muxer = {
            let secio = SecioUpgrade::new();
            let tls = TLSUpgrade::new();
            let select = SelectUpgrade::new(tls, secio);

            UpgradeMuxer::apply_outbound(client_conn, select)
        };

        task::block_on(async {

            let server = task::spawn(async move {
                let conn = server_muxer.await.expect("");
                println!("{:?}", conn);
            });

            let client = task::spawn(async move {
                let conn = client_muxer.await.expect("");
                println!("{:?}", conn);
            });

            server.await;
            client.await;
        });
    }
}



