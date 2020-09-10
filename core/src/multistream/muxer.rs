use std::{
    collections::HashMap,
    future::Future,
};
use async_trait::async_trait;

use super::{
    negotiator::{Negotiator, NegotiationError},
    ReadEx, WriteEx,
};

pub trait Stream {}

pub type BoxStream = Box<dyn Stream + Send>;

#[async_trait]
pub trait Handler<T> {
    async fn handle(&mut self, s: &mut BoxStream) -> T;
}

pub type BoxHandler<T> = Box<dyn Handler<T> + Send + Sync>;

#[async_trait]
impl<F, T, Fut> Handler<T> for F
where
    F: FnMut(&mut BoxStream) -> Fut,
    F: Send + 'static,
    Fut: Future<Output = T>,
    Fut: Send + 'static,
    T: Send + 'static,
{
    async fn handle(&mut self, s: &mut BoxStream) -> T {
        let f = self;
        f(s).await
    }
}

// type Handler<T, Fut: Future> = Box<dyn FnMut(&mut dyn Stream) -> Fut<Output = T> + Send + Sync>;

pub struct Muxer<TProto, T> {
    negotiator: Negotiator<TProto>,

    handlers: HashMap<TProto, BoxHandler<T>>,
}

impl<TProto, T> Muxer<TProto, T>
where
    TProto: AsRef<[u8]> + Clone + Eq + std::hash::Hash,
    T: Send + 'static,
{
    pub fn new() -> Self {
        Muxer {
            negotiator: Negotiator::new(),
            handlers: HashMap::new(),
        }
    }

    pub fn add_handler(&mut self, proto: TProto, handler: BoxHandler<T>)
        -> Option<BoxHandler<T>>
    {
        self.negotiator.add_protocol(proto.clone());
        self.handlers.insert(proto, handler)
    }

    pub async fn negotiate<TSocket>(&mut self, socket: TSocket)
                                    -> Result<(&mut BoxHandler<T>, TProto, TSocket), NegotiationError>
        where
            TSocket: ReadEx + WriteEx + Send + Unpin
    {
        let (proto, io) = self.negotiator.negotiate(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }

    pub async fn select_one<TSocket>(&mut self, socket: TSocket)
                                     -> Result<(&mut BoxHandler<T>, TProto, TSocket), NegotiationError>
        where
            TSocket: ReadEx + WriteEx + Send + Unpin
    {
        let (proto, io) = self.negotiator.select_one(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }
}

/*
impl<S, Fut, T, E, P, H> Muxer<P, H>
where
    S: Stream,
    Fut: Future<Output = Result<T, E>>,
    Fut: Send + 'static,
    T: Send + 'static,
    E: Error,
    P: AsRef<[u8]> + Clone + Eq + std::hash::Hash,
    H: FnMut(S) -> Fut,
{
    pub fn new() -> Self {
        Muxer {
            negotiator: Negotiator::new(),
            handlers: HashMap::new(),
        }
    }

    pub fn add_handler(&mut self, proto: P, handler: H) -> Option<H> {
        self.negotiator.add_protocol(proto.clone());
        self.handlers.insert(proto, handler)
    }

    pub async fn negotiate<TSocket>(&mut self, socket: TSocket)
        -> Result<(&mut H, P, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Send + Unpin
    {
        let (proto, io) = self.negotiator.negotiate(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }

    pub async fn select_one<TSocket>(&mut self, socket: TSocket)
        -> Result<(&mut H, P, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Send + Unpin
    {
        let (proto, io) = self.negotiator.select_one(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }
}
 */

#[cfg(test)]
mod tests {
    use std::io;
    use async_trait::async_trait;
    use async_std::task;
    use futures::channel::mpsc;
    use bytes::Bytes;

    use super::{BoxHandler, Muxer, Stream, BoxStream, Handler};
    use super::super::Memory;
    use futures::{StreamExt, SinkExt};


    struct Test(String);
    impl Stream for Test {}

    #[async_trait]
    impl Handler<String> for Test {
        async fn handle(&mut self, _s: &mut BoxStream) -> String {
            format!("/proto1 {} handler", self.0)
        }
    }

    fn get_stream() -> BoxStream {
        Box::new(Test("stream".to_string()))
    }

    fn get_handler(name: &str) -> BoxHandler<String> {
        Box::new(Test(name.to_string()))
    }

    /*
    fn get_server_proto_handler() -> BoxHandler<()> {
        Box::new(|_s: &mut BoxStream| {
            async {
                println!("/proto1 server handler");
            }
        })
    }
     */

    fn get_client_proto_handler() -> BoxHandler<&'static str> {
        Box::new(|_s: &mut BoxStream| {
            async {
                "/proto1 client handler"
            }
        })
    }

    #[test]
    fn test_muxer() {

        task::block_on(async {
            let (client, server) = Memory::pair();

            let server = task::spawn(async move {
                let mut muxer = Muxer::new();
                let duplicate = muxer.add_handler(b"/proto1", get_handler("server")).is_some();
                assert!(!duplicate, "add duplicate protocol '{}' handler", "/proto1");

                let (h, proto, _) = muxer.negotiate(server).await
                    .expect("muxer.negotiate");

                assert_eq!(proto, b"/proto1");

                let mut s = get_stream();
                let x = h.handle(&mut s).await;
                assert_eq!(x.as_str(), "/proto1 server handler")
            });

            let client = task::spawn(async move {
                let mut muxer = Muxer::new();
                let duplicate = muxer.add_handler(b"/proto1", get_client_proto_handler()).is_some();
                assert!(!duplicate, "add duplicate protocol '{}' handler", "/proto1");

                let (h, proto, _) = muxer.select_one(client).await
                    .expect("muxer.select_one");

                assert_eq!(proto, b"/proto1");

                let mut s = get_stream();
                let x = h.handle(&mut s).await;
                assert_eq!(x, "/proto1 client handler");
            });

            server.await;
            client.await;
        });
    }
}
