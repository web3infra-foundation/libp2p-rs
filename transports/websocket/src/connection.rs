use futures::{
    prelude::*,
    stream::{BoxStream, IntoAsyncRead, TryStreamExt},
};
use libp2prs_core::{either::EitherOutput, multiaddr::Multiaddr, transport::ConnectionInfo};
use quicksink::Action;
use soketto::connection;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

pub type TlsOrPlain<T> = EitherOutput<EitherOutput<TlsClientStream<T>, TlsServerStream<T>>, T>;

#[pin_project::pin_project]
pub struct Connection<T> {
    #[pin]
    reader: IntoAsyncRead<BoxStream<'static, io::Result<Vec<u8>>>>,
    #[pin]
    writer: Pin<Box<dyn Sink<Vec<u8>, Error = io::Error> + Send>>,

    local_addr: Multiaddr,
    remote_addr: Multiaddr,

    _mark: std::marker::PhantomData<T>,
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    #[allow(clippy::needless_return)]
    pub fn new(builder: connection::Builder<T>, local_addr: Multiaddr, remote_addr: Multiaddr) -> Self {
        let (tx, rx) = builder.finish();

        let stream = futures::stream::unfold(rx, move |mut rx| async move {
            let mut buf = Vec::with_capacity(1024);
            log::debug!("receiving data");
            match rx.receive_data(&mut buf).await {
                Ok(data) => match data {
                    soketto::Data::Binary(n) | soketto::Data::Text(n) => {
                        buf.truncate(n);
                        log::debug!("receive data ok: {:?}", buf);
                        return Some((Ok(buf), rx));
                    }
                },
                Err(e) => {
                    log::debug!("receive data err: {:?}", e);
                    match e {
                        connection::Error::Io(ioe) => return Some((Err(ioe), rx)),
                        connection::Error::Closed => return None,
                        _ => return Some((Err(io::Error::new(io::ErrorKind::Other, e)), rx)),
                    }
                }
            }
        });
        let stream: BoxStream<'static, io::Result<Vec<u8>>> = stream.boxed();
        let reader = stream.into_async_read();

        let sink = quicksink::make_sink(tx, move |mut tx, action: Action<Vec<u8>>| async move {
            match action {
                Action::Send(data) => {
                    log::debug!("send data: {:?}", data);
                    tx.send_binary(data).await.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
                Action::Flush => {
                    tx.flush().await.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
                Action::Close => {
                    tx.close().await.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
            }
            Ok(tx)
        });

        Connection {
            reader,
            writer: Box::pin(sink),
            local_addr,
            remote_addr,
            _mark: std::marker::PhantomData,
        }
    }
}

impl<T> AsyncRead for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.project().reader.poll_read(cx, buf)
    }
}

impl<T> AsyncWrite for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        futures::ready!(this.writer.as_mut().poll_ready(cx))?;
        let n = buf.len();
        if let Err(e) = this.writer.as_mut().start_send(buf.to_vec()) {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().writer.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().writer.poll_close(cx)
    }
}

impl<T> ConnectionInfo for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn local_multiaddr(&self) -> Multiaddr {
        self.local_addr.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.remote_addr.clone()
    }
}

pub struct TlsClientStream<T>(pub(crate) async_tls::client::TlsStream<T>);

impl<T> AsyncRead for TlsClientStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.0), cx, buf)
    }
}

impl<T> AsyncWrite for TlsClientStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

pub struct TlsServerStream<T>(pub(crate) async_tls::server::TlsStream<T>);

impl<T> AsyncRead for TlsServerStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.0), cx, buf)
    }
}

impl<T> AsyncWrite for TlsServerStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}
