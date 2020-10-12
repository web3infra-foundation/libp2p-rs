// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Noise protocol I/O.

mod framed;
pub mod handshake;

use async_trait::async_trait;
use bytes::Bytes;
use framed::{NoiseFramed, MAX_FRAME_LEN};
use futures::io::Error;
use libp2p_core::identity::Keypair;
use libp2p_core::secure_io::SecureInfo;
use libp2p_core::transport::ConnectionInfo;
use libp2p_core::{Multiaddr, PeerId, PublicKey};
use libp2p_traits::{ReadEx, WriteEx};
use log::trace;
use std::{cmp::min, fmt, io};

/// A noise session to a remote.
///
/// `T` is the type of the underlying I/O resource.
pub struct NoiseOutput<T> {
    io: NoiseFramed<T, snow::TransportState>,
    la: Multiaddr,
    ra: Multiaddr,
    recv_buffer: Bytes,
    recv_offset: usize,
    send_buffer: Vec<u8>,
    send_offset: usize,
    local_priv_key: Keypair,
    remote_pub_key: PublicKey,
}

impl<S: ConnectionInfo> ConnectionInfo for NoiseOutput<S> {
    // TODO: Now la&ra is not none in xx, but ought to find a method to fix it
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl<T> fmt::Debug for NoiseOutput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoiseOutput").field("io", &self.io).finish()
    }
}

impl<T> NoiseOutput<T> {
    fn new(io: NoiseFramed<T, snow::TransportState>, keypair: Keypair) -> Self {
        let remote_pub_key = keypair.public();
        NoiseOutput {
            io,
            la: Multiaddr::empty(),
            ra: Multiaddr::empty(),
            recv_buffer: Bytes::new(),
            recv_offset: 0,
            send_buffer: Vec::new(),
            send_offset: 0,
            local_priv_key: keypair,
            remote_pub_key,
        }
    }

    pub fn add_addr(&mut self, la: Multiaddr, ra: Multiaddr) {
        self.la = la;
        self.ra = ra;
    }
}

impl<S> SecureInfo for NoiseOutput<S> {
    fn local_peer(&self) -> PeerId {
        self.local_priv_key.clone().public().into_peer_id()
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_pub_key.clone().into_peer_id()
    }

    fn local_priv_key(&self) -> Keypair {
        self.local_priv_key.clone()
    }

    fn remote_pub_key(&self) -> PublicKey {
        self.remote_pub_key.clone()
    }
}

#[async_trait]
impl<T: ReadEx + WriteEx + Send + Unpin> ReadEx for NoiseOutput<T> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let len = self.recv_buffer.len();
            let off = self.recv_offset;
            if len > 0 {
                let n = min(len - off, buf.len());
                buf[..n].copy_from_slice(&self.recv_buffer[off..off + n]);
                trace!("read: copied {}/{} bytes", off + n, len);
                self.recv_offset += n;
                if len == self.recv_offset {
                    trace!("read: frame consumed");
                    // Drop the existing view so `NoiseFramed` can reuse
                    // the buffer when polling for the next frame below.
                    self.recv_buffer = Bytes::new();
                }
                return Ok(n);
            }

            match self.io.next().await {
                Some(Ok(frame)) => {
                    self.recv_buffer = frame;
                    self.recv_offset = 0;
                }
                None => return Ok(0),
                Some(Err(e)) => return Err(e),
            }
        }
    }
}

#[async_trait]
impl<T: WriteEx + ReadEx + Send + Unpin> WriteEx for NoiseOutput<T> {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, Error> {
        // let mut io = Pin::new(&mut self.io);
        let frame_buf = &mut self.send_buffer;

        // The MAX_FRAME_LEN is the maximum buffer size before a frame must be sent.
        if self.send_offset == MAX_FRAME_LEN {
            trace!("write: sending {} bytes", MAX_FRAME_LEN);
            // ready!(io.as_mut().poll_ready(cx))?;
            // self.io.ready2().await;
            self.io.send2(&frame_buf).await?;
            self.send_offset = 0;
        }

        let off = self.send_offset;
        let n = min(MAX_FRAME_LEN, off.saturating_add(buf.len()));
        self.send_buffer.resize(n, 0u8);
        let n = min(MAX_FRAME_LEN - off, buf.len());
        self.send_buffer[off..off + n].copy_from_slice(&buf[..n]);
        self.send_offset += n;
        trace!("write: buffered {} bytes", self.send_offset);

        self.flush2().await?;

        Ok(n)
    }

    async fn flush2(&mut self) -> Result<(), Error> {
        let frame_buf = &mut self.send_buffer;

        // Check if there is still one more frame to send.
        if self.send_offset > 0 {
            self.io.ready2().await?;
            // ready!(io.as_mut().poll_ready(cx))?;
            trace!("flush: sending {} bytes", self.send_offset);
            self.io.send2(&frame_buf).await?;
            self.send_offset = 0;
        }

        self.io.flush2().await
    }

    async fn close2(&mut self) -> Result<(), Error> {
        // ready!(self.as_mut().poll_flush(cx))?;
        // Pin::new(&mut self.io).poll_close(cx)
        self.flush2().await?;
        self.io.close2().await
    }
}
