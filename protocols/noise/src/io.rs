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

//! Noise protocol I/O.

mod framed;
pub mod handshake;
mod secure_stream;

use crate::io::secure_stream::{NoiseSecureStream, NoiseSecureStreamReader, NoiseSecureStreamWriter};
use async_trait::async_trait;
use libp2prs_core::identity::Keypair;
use libp2prs_core::secure_io::SecureInfo;
use libp2prs_core::transport::ConnectionInfo;
use libp2prs_core::{Multiaddr, PeerId, PublicKey};
use libp2prs_traits::{ReadEx, SplitEx, SplittableReadWrite, WriteEx};
use std::io;

/// A noise session to a remote.
///
/// `T` is the type of the underlying I/O resource.
pub struct NoiseOutput<T: SplitEx> {
    pub io: NoiseSecureStream<T::Reader, T::Writer>,
    la: Multiaddr,
    ra: Multiaddr,
    local_priv_key: Keypair,
    remote_pub_key: PublicKey,
}

impl<T: ConnectionInfo + SplitEx> ConnectionInfo for NoiseOutput<T> {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}
//
// impl<T> fmt::Debug for NoiseOutput<T> {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.debug_struct("NoiseOutput").field("io", &self.io).finish()
//     }
// }

impl<T: SplittableReadWrite> NoiseOutput<T> {
    fn new(io: NoiseSecureStream<T::Reader, T::Writer>, keypair: Keypair) -> Self {
        let remote_pub_key = keypair.public();
        NoiseOutput {
            io,
            la: Multiaddr::empty(),
            ra: Multiaddr::empty(),
            local_priv_key: keypair,
            remote_pub_key,
        }
    }

    pub fn add_addr(&mut self, la: Multiaddr, ra: Multiaddr) {
        self.la = la;
        self.ra = ra;
    }
}

impl<T: SplitEx> SecureInfo for NoiseOutput<T> {
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
impl<T: SplittableReadWrite> ReadEx for NoiseOutput<T> {
    async fn read2(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.io.read2(buf).await
    }
}

#[async_trait]
impl<T: SplittableReadWrite> WriteEx for NoiseOutput<T> {
    async fn write2(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.io.write2(buf).await
    }

    async fn flush2(&mut self) -> io::Result<()> {
        self.io.flush2().await
    }

    async fn close2(&mut self) -> io::Result<()> {
        self.io.close2().await
    }
}

impl<S: SplittableReadWrite> SplitEx for NoiseOutput<S> {
    type Reader = NoiseSecureStreamReader<S::Reader>;
    type Writer = NoiseSecureStreamWriter<S::Writer>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (r, w) = self.io.split();
        (r, w)
    }
}
