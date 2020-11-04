use async_trait::async_trait;
use futures::prelude::*;
use libp2prs_core::either::AsyncEitherOutput;
use libp2prs_core::multiaddr::Multiaddr;
use libp2prs_core::transport::ConnectionInfo;
use libp2prs_traits::{ReadEx, SplitEx, WriteEx};
use soketto::connection;
use std::{
    io::{self, Error},
    pin::Pin,
    task::{Context, Poll},
};

pub type TlsOrPlain<T> = AsyncEitherOutput<AsyncEitherOutput<TlsClientStream<T>, TlsServerStream<T>>, T>;

pub struct ConnectionReader<R> {
    recvier: connection::Receiver<R>,
    recv_buf: Vec<u8>,
}

impl<R> ConnectionReader<R> {
    #[inline]
    fn drain(&mut self, buf: &mut [u8]) -> usize {
        // Return zero if there is no data remaining in the internal buffer.
        if self.recv_buf.is_empty() {
            return 0;
        }

        // calculate number of bytes that we can copy
        let n = ::std::cmp::min(buf.len(), self.recv_buf.len());

        // Copy data to the output buffer
        buf[..n].copy_from_slice(self.recv_buf[..n].as_ref());

        // drain n bytes of recv_buf
        self.recv_buf = self.recv_buf.split_off(n);

        n
    }
}

#[async_trait]
impl<R: AsyncRead + AsyncWrite + Unpin + Send> ReadEx for ConnectionReader<R> {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // when there is something in recv_buffer
        let copied = self.drain(buf);
        if copied > 0 {
            log::debug!("drain recv buffer data size: {:?}", copied);
            return Ok(copied);
        }
        let mut v = Vec::with_capacity(buf.len());
        match self.recvier.receive_data(&mut v).await.map_err(|e| {
            log::info!("{:?}", e);
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })? {
            soketto::Data::Binary(n) | soketto::Data::Text(n) => {
                if buf.len() >= n {
                    buf[..n].copy_from_slice(v.as_ref());
                    Ok(n)
                } else {
                    // fill internal recv buffer
                    self.recv_buf = v;
                    // drain for input buffer
                    let copied = self.drain(buf);
                    Ok(copied)
                }
            }
        }
    }
}

pub struct ConnectionWriter<W> {
    sender: connection::Sender<W>,
}

#[async_trait]
impl<W: AsyncRead + AsyncWrite + Unpin + Send> WriteEx for ConnectionWriter<W> {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        let n = buf.len();
        self.sender
            .send_binary(buf)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(n)
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        self.sender
            .flush()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.sender
            .close()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }
}

pub struct Connection<T> {
    reader: ConnectionReader<T>,
    writer: ConnectionWriter<T>,

    local_addr: Multiaddr,
    remote_addr: Multiaddr,
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    pub fn new(builder: connection::Builder<T>, local_addr: Multiaddr, remote_addr: Multiaddr) -> Self {
        let (tx, rx) = builder.finish();
        Connection {
            reader: ConnectionReader {
                recvier: rx,
                recv_buf: Vec::default(),
            },
            writer: ConnectionWriter { sender: tx },
            local_addr,
            remote_addr,
        }
    }
}

#[async_trait]
impl<T> ReadEx for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        log::debug!("read from connection, buf len {}", buf.len());
        self.reader.read2(buf).await
    }
}

#[async_trait]
impl<T> WriteEx for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.writer.write2(buf).await
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        self.writer.flush2().await
    }

    async fn close2(&mut self) -> Result<(), Error> {
        self.writer.close2().await
    }
}

impl<T> SplitEx for Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Reader = ConnectionReader<T>;
    type Writer = ConnectionWriter<T>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (self.reader, self.writer)
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
        Pin::new(&mut self.0).poll_read(cx, buf)
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
        Pin::new(&mut self.0).poll_read(cx, buf)
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
