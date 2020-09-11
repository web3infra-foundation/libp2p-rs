use super::{ReadEx, WriteEx, Upgrader};

pub mod secio {
    use std::iter;
    use async_trait::async_trait;

    use super::{ReadEx, WriteEx, Upgrader};

    pub struct SecioUpgrade;

    impl SecioUpgrade {
        pub fn new() -> Self {
            SecioUpgrade
        }
    }

    #[async_trait]
    impl<TSocket: ReadEx + WriteEx + Send+ Unpin + 'static> Upgrader<TSocket> for SecioUpgrade {
        type Output = SecioOutput<TSocket>;
        type Proto = &'static str;
        type ProtoIter = iter::Once<&'static str>;

        async fn upgrade_inbound(self, socket: TSocket, proto: &'static str) -> Self::Output {
            SecioOutput(socket)
        }

        async fn upgrade_outbound(self, socket: TSocket, proto: &'static str) -> Self::Output {
            SecioOutput(socket)
        }

        fn protocols(&self) -> Self::ProtoIter {
            iter::once("/secio/1.0.0")
        }
    }

    #[derive(Debug)]
    pub struct SecioOutput<TSocket>(TSocket);
}

pub mod tls {
    use std::iter;
    use async_trait::async_trait;

    use super::{ReadEx, WriteEx, Upgrader};

    pub struct TLSUpgrade;

    impl TLSUpgrade {
        pub fn new() -> Self {
            TLSUpgrade
        }
    }

    #[async_trait]
    impl<TSocket: ReadEx + WriteEx + Send+ Unpin + 'static> Upgrader<TSocket> for TLSUpgrade {
        type Output = TLSOutput<TSocket>;
        type Proto = &'static str;
        type ProtoIter = iter::Once<&'static str>;

        async fn upgrade_inbound(self, socket: TSocket, proto: &'static str) -> Self::Output {
            TLSOutput(socket)
        }

        async fn upgrade_outbound(self, socket: TSocket, proto: &'static str) -> Self::Output {
            TLSOutput(socket)
        }

        fn protocols(&self) -> Self::ProtoIter {
            iter::once("/tls/1.0.0")
        }
    }

    #[derive(Debug)]
    pub struct TLSOutput<TSocket>(TSocket);
}