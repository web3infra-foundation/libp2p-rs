use async_tls::{client, server};
use async_trait::async_trait;
use futures::prelude::*;
use futures::{ready, stream::BoxStream};
use libp2p_core::transport::ConnectionInfo;
use libp2p_core::{either::EitherOutput, multiaddr::Multiaddr};
use libp2p_tcp::TcpTransStream;
use libp2p_traits::{ReadEx, WriteEx};
use log::trace;
use soketto::connection;
use std::{convert::TryInto, fmt, io, mem, pin::Pin, task::Context, task::Poll};

//type SokettoReceiver<T>=soketto::Receiver<EitherOutput<EitherOutput<client::TlsStream<T>, server::TlsStream<T>>, T>>;
//type SokettoSender<T>=soketto::Sender<EitherOutput<EitherOutput<client::TlsStream<T>, server::TlsStream<T>>, T>>;
pub(crate) type TlsOrPlain<T> = EitherOutput<EitherOutput<client::TlsStream<T>, server::TlsStream<T>>, T>;

/// The websocket connection.
pub struct Connection<T> {
    //reader:SokettoReceiver<T>,
    //writer:SokettoSender<T>,
    inner: TcpTransStream,
    receiver: BoxStream<'static, Result<IncomingData, connection::Error>>,
    sender: Pin<Box<dyn Sink<OutgoingData, Error = connection::Error> + Send>>,
    _marker: std::marker::PhantomData<T>,
    la: Multiaddr,
    ra: Multiaddr,
    buf: Vec<u8>,
}

//impl ConnectionInfo
impl<T: ConnectionInfo> ConnectionInfo for Connection<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

/// Data received over the websocket connection.
#[derive(Debug, Clone, PartialEq)]
pub enum IncomingData {
    /// Binary application data.
    Binary(Vec<u8>),
    /// UTF-8 encoded application data.
    Text(Vec<u8>),
    /// PONG control frame data.
    Pong(Vec<u8>),
}

#[allow(dead_code)]
impl IncomingData {
    pub fn is_data(&self) -> bool {
        self.is_binary() || self.is_text()
    }

    pub fn is_binary(&self) -> bool {
        if let IncomingData::Binary(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_text(&self) -> bool {
        if let IncomingData::Text(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_pong(&self) -> bool {
        if let IncomingData::Pong(_) = self {
            true
        } else {
            false
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            IncomingData::Binary(d) => d,
            IncomingData::Text(d) => d,
            IncomingData::Pong(d) => d,
        }
    }
}

impl AsRef<[u8]> for IncomingData {
    fn as_ref(&self) -> &[u8] {
        match self {
            IncomingData::Binary(d) => d,
            IncomingData::Text(d) => d,
            IncomingData::Pong(d) => d,
        }
    }
}

/// Data sent over the websocket connection.
#[derive(Debug, Clone)]
pub enum OutgoingData {
    /// Send some bytes.
    Binary(Vec<u8>),
    /// Send a PING message.
    Ping(Vec<u8>),
    /// Send an unsolicited PONG message.
    /// (Incoming PINGs are answered automatically.)
    Pong(Vec<u8>),
}

impl<T> fmt::Debug for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Connection")
    }
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    pub(crate) fn new(inner: TcpTransStream, builder: connection::Builder<TlsOrPlain<T>>, la: Multiaddr, ra: Multiaddr) -> Self {
        let (sender, receiver) = builder.finish();
        let sink = quicksink::make_sink(sender, |mut sender, action| async move {
            match action {
                quicksink::Action::Send(OutgoingData::Binary(x)) => sender.send_binary_mut(x).await?,
                quicksink::Action::Send(OutgoingData::Ping(x)) => {
                    let data = x[..]
                        .try_into()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PING data must be < 126 bytes"))?;
                    sender.send_ping(data).await?
                }
                quicksink::Action::Send(OutgoingData::Pong(x)) => {
                    let data = x[..]
                        .try_into()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PONG data must be < 126 bytes"))?;
                    sender.send_pong(data).await?
                }
                quicksink::Action::Flush => sender.flush().await?,
                quicksink::Action::Close => sender.close().await?,
            }
            Ok(sender)
        });

        let stream = stream::unfold((Vec::new(), receiver), |(mut data, mut receiver)| async {
            match receiver.receive(&mut data).await {
                Ok(soketto::Incoming::Data(soketto::Data::Text(_))) => {
                    Some((Ok(IncomingData::Text(mem::take(&mut data))), (data, receiver)))
                }
                Ok(soketto::Incoming::Data(soketto::Data::Binary(_))) => {
                    Some((Ok(IncomingData::Binary(mem::take(&mut data))), (data, receiver)))
                }
                Ok(soketto::Incoming::Pong(pong)) => Some((Ok(IncomingData::Pong(Vec::from(pong))), (data, receiver))),
                Err(connection::Error::Closed) => None,
                Err(e) => Some((Err(e), (data, receiver))),
            }
        });
        Connection {
            inner,
            receiver: stream.boxed(),
            sender: Box::pin(sink),
            _marker: std::marker::PhantomData,
            la,
            ra,
            buf: Vec::with_capacity(128),
        }
    }

    /// Send binary application data to the remote.
    pub fn send_data(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Binary(data))
    }

    /// Send a PING to the remote.
    pub fn send_ping(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Ping(data))
    }

    /// Send an unsolicited PONG to the remote.
    pub fn send_pong(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Pong(data))
    }
}

impl<T> Stream for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Item = io::Result<IncomingData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let item = ready!(self.receiver.poll_next_unpin(cx));
        let item = item.map(|result| result.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        Poll::Ready(item)
    }
}

impl<T> Sink<OutgoingData> for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_ready(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn start_send(mut self: Pin<&mut Self>, item: OutgoingData) -> io::Result<()> {
        Pin::new(&mut self.sender)
            .start_send(item)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_flush(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_close(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

#[async_trait]
impl<T> ReadEx for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        // return the buffer first
        let n = buf.len();
        if !self.buf.is_empty() {
            buf.copy_from_slice(&self.buf[0..n]);
            self.buf.drain(0..n);
            trace!("read buf : {:?}", buf);
            return Ok(n);
        }
        debug_assert!(self.buf.is_empty());

        let item = self.receiver.next().await;
        let item = item.map(|result| result.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        match item {
            Some(Ok(IncomingData::Binary(bytes))) => {
                trace!("recv IncomingData::Binary = {:?}", bytes);
                self.buf.resize_with(bytes.len(), Default::default);
                self.buf.copy_from_slice(&bytes[..]);
                buf.copy_from_slice(&self.buf[0..n]);
                self.buf.drain(0..n);
                Ok(n)
            }
            Some(Err(e)) => Err(e),
            //todo: other IncomingData
            _ => Ok(0),
        }
    }
}

#[async_trait]
impl<T> WriteEx for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        trace!("write buf : {:?}", buf);
        let _ = self.send_data(buf.to_vec()).await;
        Ok(buf.len())
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        //todo: call ws connection Sender flush
        self.inner.flush2().await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        //todo: call ws connection Sender close
        self.inner.close2().await
    }
}
