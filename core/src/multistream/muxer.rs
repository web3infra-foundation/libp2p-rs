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

use async_trait::async_trait;
use std::{collections::HashMap, future::Future};

use super::{
    negotiator::{NegotiationError, Negotiator},
    ReadEx, WriteEx,
};

pub trait Stream {}

pub type BoxStream = Box<dyn Stream + Send>;

#[async_trait]
pub trait Handler<T> {
    async fn handle(&mut self, s: &mut BoxStream) -> T;
}

pub type IProtocolHandler<T> = Box<dyn Handler<T> + Send + Sync>;

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

    handlers: HashMap<TProto, IProtocolHandler<T>>,
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

    pub fn add_handler(&mut self, proto: TProto, handler: IProtocolHandler<T>) -> Option<IProtocolHandler<T>> {
        self.negotiator.add_protocol(proto.clone()).expect("protocol duplicate");
        self.handlers.insert(proto, handler)
    }

    pub async fn negotiate<TSocket>(
        &mut self,
        socket: TSocket,
    ) -> Result<(&mut IProtocolHandler<T>, TProto, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Unpin,
    {
        let (proto, io) = self.negotiator.negotiate(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }

    pub async fn select_one<TSocket>(
        &mut self,
        socket: TSocket,
    ) -> Result<(&mut IProtocolHandler<T>, TProto, TSocket), NegotiationError>
    where
        TSocket: ReadEx + WriteEx + Unpin,
    {
        let (proto, io) = self.negotiator.select_one(socket).await?;
        let h = self.handlers.get_mut(&proto).expect("get handler");
        Ok((h, proto, io))
    }
}

impl<TProto, T> Default for Muxer<TProto, T>
where
    TProto: AsRef<[u8]> + Clone + Eq + std::hash::Hash,
    T: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use async_std::task;
    use async_trait::async_trait;

    use super::super::Memory;
    use super::{BoxStream, Handler, IProtocolHandler, Muxer, Stream};

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

    fn get_handler(name: &str) -> IProtocolHandler<String> {
        Box::new(Test(name.to_string()))
    }

    /*
    fn get_server_proto_handler() -> IProtocolHandler<()> {
        Box::new(|_s: &mut BoxStream| {
            async {
                println!("/proto1 server handler");
            }
        })
    }
     */

    fn get_client_proto_handler() -> IProtocolHandler<&'static str> {
        Box::new(|_s: &mut BoxStream| async { "/proto1 client handler" })
    }

    #[test]
    fn test_muxer() {
        task::block_on(async {
            let (client, server) = Memory::pair();

            let server = task::spawn(async move {
                let mut muxer = Muxer::new();
                let duplicate = muxer.add_handler(b"/proto1", get_handler("server")).is_some();
                assert!(!duplicate, "add duplicate protocol '{}' handler", "/proto1");

                let (h, proto, _) = muxer.negotiate(server).await.expect("muxer.negotiate");

                assert_eq!(proto, b"/proto1");

                let mut s = get_stream();
                let x = h.handle(&mut s).await;
                assert_eq!(x.as_str(), "/proto1 server handler")
            });

            let client = task::spawn(async move {
                let mut muxer = Muxer::new();
                let duplicate = muxer.add_handler(b"/proto1", get_client_proto_handler()).is_some();
                assert!(!duplicate, "add duplicate protocol '{}' handler", "/proto1");

                let (h, proto, _) = muxer.select_one(client).await.expect("muxer.select_one");

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
