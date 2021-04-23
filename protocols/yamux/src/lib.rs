// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Implements the Yamux multiplexing protocol for libp2p, see also the
//! [specification](https://github.com/hashicorp/yamux/blob/master/spec.md).

use futures::channel::mpsc;
use futures::future::BoxFuture;
use futures::{prelude::*, stream::StreamExt, FutureExt, SinkExt};
use libp2prs_core::identity::{Keypair, PublicKey};
use libp2prs_core::muxing::{IReadWrite, IStreamMuxer, ReadWriteEx, StreamInfo, StreamMuxer, StreamMuxerEx};
use libp2prs_core::secure_io::SecureInfo;
use libp2prs_core::transport::{ConnectionInfo, TransportError};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use libp2prs_core::{Multiaddr, PeerId};
use parking_lot::Mutex;
use std::sync::Arc;
use std::{
    fmt, io,
    pin::Pin,
    task::{Context, Poll},
};

type YRet = Result<yamux::Stream, yamux::ConnectionError>;

/// A Yamux connection.
// #[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct Yamux<T> {
    ///
    incoming: Arc<Mutex<Option<(yamux::Connection<T>, mpsc::UnboundedSender<YRet>)>>>,
    ///
    accepted: Arc<futures::lock::Mutex<mpsc::UnboundedReceiver<YRet>>>,
    /// Handle to control the connection.
    control: yamux::Control,
    /// The local multiaddr of this connection
    pub la: Multiaddr,
    /// The remote multiaddr of this connection
    pub ra: Multiaddr,
    /// The private key of the local
    pub local_priv_key: Keypair,
    /// For convenience, the local peer ID, generated from local pub key
    pub local_peer_id: PeerId,
    /// The public key of the remote.
    pub remote_pub_key: PublicKey,
    /// For convenience, put a PeerId here, which is actually calculated from remote_key
    pub remote_peer_id: PeerId,
}

impl<T> Clone for Yamux<T> {
    fn clone(&self) -> Self {
        Yamux {
            incoming: self.incoming.clone(),
            accepted: self.accepted.clone(),
            control: self.control.clone(),
            la: self.la.clone(),
            ra: self.ra.clone(),
            local_priv_key: self.local_priv_key.clone(),
            local_peer_id: self.local_peer_id,
            remote_pub_key: self.remote_pub_key.clone(),
            remote_peer_id: self.remote_peer_id,
        }
    }
}

impl<S> fmt::Debug for Yamux<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Yamux")
    }
}

impl<T> Yamux<T>
where
    T: ConnectionInfo + SecureInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new Yamux connection.
    fn new(io: T, mut cfg: yamux::Config, mode: yamux::Mode) -> Self {
        cfg.set_read_after_close(false);

        let local_priv_key = io.local_priv_key();
        let remote_pub_key = io.remote_pub_key();
        let local_peer_id = io.local_peer();
        let remote_peer_id = io.remote_peer();
        let la = io.local_multiaddr();
        let ra = io.remote_multiaddr();

        let conn = yamux::Connection::new(io, cfg, mode);
        let (sender, accepted) = mpsc::unbounded();

        let ctrl = conn.control();
        Yamux {
            incoming: Arc::new(Mutex::new(Some((conn, sender)))),
            accepted: Arc::new(futures::lock::Mutex::new(accepted)),
            control: ctrl,
            la,
            ra,
            local_priv_key,
            local_peer_id,
            remote_pub_key,
            remote_peer_id,
        }
    }
}

impl<T> SecureInfo for Yamux<T> {
    fn local_peer(&self) -> PeerId {
        self.local_peer_id
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_peer_id
    }

    fn local_priv_key(&self) -> Keypair {
        self.local_priv_key.clone()
    }

    fn remote_pub_key(&self) -> PublicKey {
        self.remote_pub_key.clone()
    }
}

impl<T: Send> ConnectionInfo for Yamux<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }
    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> StreamMuxerEx for Yamux<T> {}

#[async_trait::async_trait]
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> StreamMuxer for Yamux<T> {
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError> {
        let s = self.control.open_stream().await.map_err(map_yamux_err)?;
        log::trace!("a new outbound substream {:?} opened for yamux... ", s);
        Ok(Box::new(Stream(s)))
    }

    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError> {
        if let Some(s) = self.accepted.lock().await.next().await {
            let stream = s.map_err(map_yamux_err)?;
            log::trace!("a new inbound substream {:?} accepted for yamux...", stream);
            return Ok(Box::new(Stream(stream)));
        }
        Err(TransportError::StreamMuxerError(Box::new(yamux::ConnectionError::Closed)))
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.control.close().await.map_err(map_yamux_err)
    }

    fn task(&mut self) -> Option<BoxFuture<'static, ()>> {
        self.incoming.lock().take().map(|(mut conn, mut sender)| {
            async move {
                loop {
                    match conn.next_stream().await {
                        Ok(Some(s)) => {
                            if let Err(e) = sender.send(Ok(s)).await {
                                if e.is_disconnected() {
                                    break;
                                }
                                log::warn!("{:?}", e);
                            }
                        }
                        Ok(None) => {
                            log::info!("{:?} background-task exiting...", conn);
                            if let Err(e) = sender.send(Err(yamux::ConnectionError::Closed)).await {
                                log::warn!("{:?}", e);
                            }
                            break;
                        }
                        Err(e) => {
                            if let Err(err) = sender.send(Err(e)).await {
                                log::warn!("{:?}", err);
                            }
                            break;
                        }
                    }
                }
            }
            .boxed()
        })
    }

    fn box_clone(&self) -> IStreamMuxer {
        Box::new(Clone::clone(self))
    }
}

#[pin_project::pin_project]
#[derive(Debug)]
pub struct Stream(#[pin] yamux::Stream);

#[async_trait::async_trait]
impl ReadWriteEx for Stream {
    fn box_clone(&self) -> IReadWrite {
        unimplemented!()
    }
}

impl StreamInfo for Stream {
    fn id(&self) -> usize {
        self.0.id().val() as usize
    }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        // poll_read returns Poll:Ready(Ok(0)) means that the stream is closed,
        // we converted to an Eof error return
        match futures::ready!(self.project().0.poll_read(cx, buf)) {
            Ok(n) => {
                if n == 0 {
                    Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()))
                } else {
                    Poll::Ready(Ok(n))
                }
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.project().0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().0.poll_close(cx)
    }
}

/// The yamux configuration.
#[derive(Clone)]
pub struct Config {
    inner: yamux::Config,
    mode: Option<yamux::Mode>,
}

/// The window update mode determines when window updates are
/// sent to the remote, giving it new credit to send more data.
pub struct WindowUpdateMode(yamux::WindowUpdateMode);

impl WindowUpdateMode {
    /// The window update mode whereby the remote is given
    /// new credit via a window update whenever the current
    /// receive window is exhausted when data is received,
    /// i.e. this mode cannot exert back-pressure from application
    /// code that is slow to read from a substream.
    ///
    /// > **Note**: The receive buffer may overflow with this
    /// > strategy if the receiver is too slow in reading the
    /// > data from the buffer. The maximum receive buffer
    /// > size must be tuned appropriately for the desired
    /// > throughput and level of tolerance for (temporarily)
    /// > slow receivers.
    pub fn on_receive() -> Self {
        WindowUpdateMode(yamux::WindowUpdateMode::OnReceive)
    }

    /// The window update mode whereby the remote is given new
    /// credit only when the current receive window is exhausted
    /// when data is read from the substream's receive buffer,
    /// i.e. application code that is slow to read from a substream
    /// exerts back-pressure on the remote.
    ///
    /// > **Note**: If the receive window of a substream on
    /// > both peers is exhausted and both peers are blocked on
    /// > sending data before reading from the stream, a deadlock
    /// > occurs. To avoid this situation, reading from a substream
    /// > should never be blocked on writing to the same substream.
    ///
    /// > **Note**: With this strategy, there is usually no point in the
    /// > receive buffer being larger than the window size.
    pub fn on_read() -> Self {
        WindowUpdateMode(yamux::WindowUpdateMode::OnRead)
    }
}

impl Config {
    /// Creates a new Yamux `Config` do not specify mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new Yamux `Config` in client mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    pub fn client() -> Self {
        let mut cfg = Self::default();
        cfg.mode = Some(yamux::Mode::Client);
        cfg
    }

    /// Creates a new Yamux `Config` in server mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    pub fn server() -> Self {
        let mut cfg = Self::default();
        cfg.mode = Some(yamux::Mode::Server);
        cfg
    }

    /// Sets the size (in bytes) of the receive window per substream.
    pub fn set_receive_window_size(&mut self, num_bytes: u32) -> &mut Self {
        self.inner.set_receive_window(num_bytes);
        self
    }

    /// Sets the maximum size (in bytes) of the receive buffer per substream.
    pub fn set_max_buffer_size(&mut self, num_bytes: usize) -> &mut Self {
        self.inner.set_max_buffer_size(num_bytes);
        self
    }

    /// Sets the maximum number of concurrent substreams.
    pub fn set_max_num_streams(&mut self, num_streams: usize) -> &mut Self {
        self.inner.set_max_num_streams(num_streams);
        self
    }

    /// Sets the window update mode that determines when the remote
    /// is given new credit for sending more data.
    pub fn set_window_update_mode(&mut self, mode: WindowUpdateMode) -> &mut Self {
        self.inner.set_window_update_mode(mode.0);
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut inner = yamux::Config::default();
        // For conformity with mplex, read-after-close on a multiplexed
        // connection is never permitted and not configurable.
        inner.set_read_after_close(false);
        Config { inner, mode: None }
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec![b"/yamux/1.0.0"]
    }
}

#[async_trait::async_trait]
impl<C> Upgrader<C> for Config
where
    C: ConnectionInfo + SecureInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Yamux<C>;

    async fn upgrade_inbound(self, socket: C, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let mode = self.mode.unwrap_or(yamux::Mode::Server);
        Ok(Yamux::new(socket, self.inner, mode))
    }

    async fn upgrade_outbound(self, socket: C, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        let mode = self.mode.unwrap_or(yamux::Mode::Client);
        Ok(Yamux::new(socket, self.inner, mode))
    }
}

fn map_yamux_err(e: yamux::ConnectionError) -> TransportError {
    TransportError::StreamMuxerError(Box::new(e))
}
