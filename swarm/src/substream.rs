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

//! Logical Substream for peer-to-peer communication.
//!
//! [`Substream`] is opened via [`Connection`]'s stream muxer and then upgraded with the
//! specified protocols.
//!

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::SinkExt;
use std::sync::Arc;
use std::{fmt, io};

use libp2prs_core::muxing::IReadWrite;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_runtime::task;
use libp2prs_traits::{ReadEx, WriteEx};

use crate::connection::{ConnectionId, Direction};
use crate::control::SwarmControlCmd;
use crate::metrics::metric::Metric;
use crate::ProtocolId;

/// The Id of sub stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

#[derive(Debug)]
pub struct SubstreamInfo {
    /// The protocol of the sub stream.
    protocol: ProtocolId,
    /// The direction of the sub stream.
    dir: Direction,
}

#[derive(Debug)]
pub(crate) struct ConnectInfo {
    /// Local peer's multiaddr.
    pub(crate) la: Multiaddr,
    /// Remote peer's multiaddr.
    pub(crate) ra: Multiaddr,
    /// Remote peer's peerid.
    pub(crate) rpid: PeerId,
}

#[derive(Debug)]
struct SubstreamMeta {
    /// The protocol of the sub stream.
    protocol: ProtocolId,
    /// The direction of the sub stream.
    dir: Direction,
    /// The connection ID of the sub stream
    /// It can be used to back track to the stream muxer.
    cid: ConnectionId,
    /// Connection info
    ci: ConnectInfo,
}

/// Substream is the logical channel for the p2p connection.
/// SubstreamMeta contains the meta information of the substream and IReadWrite
/// provides the I/O operation to Substream.
#[derive(Clone)]
pub struct Substream {
    /// The inner sub stream, created by the StreamMuxer
    inner: Option<IReadWrite>,
    /// The inner information of the sub-stream
    info: Arc<SubstreamMeta>,
    /// The control channel for closing stream
    ctrl: mpsc::Sender<SwarmControlCmd>,
    /// The statistics of the substream
    metric: Arc<Metric>,
}

impl fmt::Debug for Substream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Substream")
            .field("inner", &self.inner)
            .field("protocol", &self.info.protocol)
            .field("dir", &self.info.dir)
            .field("cid", &self.info.cid)
            .finish()
    }
}

// Note that we spawn a runtime to close Substream, since Rust doesn't support Async Destructor yet
impl Drop for Substream {
    fn drop(&mut self) {
        let inner = self.inner.take();
        if let Some(mut inner) = inner {
            let cid = self.cid();
            let sid = StreamId(inner.id());
            let mut s = self.ctrl.clone();
            log::debug!("garbage collecting stream {:?}/{:?} {:?}", cid, sid, self.protocol());

            task::spawn(async move {
                let _ = s.send(SwarmControlCmd::CloseStream(cid, sid)).await;
                let _ = inner.close2().await;
            });
        }
    }
}

impl Substream {
    pub(crate) fn new(
        inner: IReadWrite,
        metric: Arc<Metric>,
        dir: Direction,
        protocol: ProtocolId,
        cid: ConnectionId,
        ci: ConnectInfo,
        ctrl: mpsc::Sender<SwarmControlCmd>,
    ) -> Self {
        Self {
            inner: Some(inner),
            info: Arc::new(SubstreamMeta { protocol, dir, cid, ci }),
            ctrl,
            metric,
        }
    }
    /// For internal test only
    #[allow(dead_code)]
    pub(crate) fn new_with_default(inner: IReadWrite) -> Self {
        let protocol = ProtocolId::from(b"/test" as &[u8]);
        let dir = Direction::Outbound;
        let cid = ConnectionId::default();
        let ci = ConnectInfo {
            la: Multiaddr::empty(),
            ra: Multiaddr::empty(),
            rpid: PeerId::random(),
        };
        let (ctrl, _) = mpsc::channel(0);
        let metric = Arc::new(Metric::new());
        Self {
            inner: Some(inner),
            info: Arc::new(SubstreamMeta { protocol, dir, cid, ci }),
            ctrl,
            metric,
        }
    }
    /// Builds a SubstreamView struct.
    pub fn to_view(&self) -> SubstreamView {
        SubstreamView {
            cid: self.cid(),
            id: self.id(),
            protocol: self.protocol().clone(),
            dir: self.dir(),
        }
    }
    /// Returns the protocol of the sub stream.
    pub fn protocol(&self) -> &ProtocolId {
        &self.info.protocol
    }
    /// Returns the direction of the sub stream.
    pub fn dir(&self) -> Direction {
        self.info.dir
    }
    /// Returns the connection id of the sub stream.
    pub fn cid(&self) -> ConnectionId {
        self.info.cid
    }
    /// Returns the sub stream Id.
    pub fn id(&self) -> StreamId {
        StreamId(self.inner.as_ref().expect("already closed?").id())
    }
    /// Returns the remote multiaddr of the sub stream.
    pub fn remote_multiaddr(&self) -> Multiaddr {
        self.info.ci.ra.clone()
    }
    /// Returns the remote multiaddr of the sub stream.
    pub fn local_multiaddr(&self) -> Multiaddr {
        self.info.ci.la.clone()
    }
    /// Returns the remote multiaddr of the sub stream.
    pub fn remote_peer(&self) -> PeerId {
        self.info.ci.rpid
    }
    /// Returns the info of the sub stream.
    pub fn info(&self) -> SubstreamInfo {
        SubstreamInfo {
            protocol: self.protocol().clone(),
            dir: self.dir(),
        }
    }
}

#[async_trait]
impl ReadEx for Substream {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.as_mut().expect("already closed?").read2(buf).await.map(|n| {
            self.metric.log_recv_msg(n);
            self.metric.log_recv_stream(self.protocol(), n, &self.info.ci.rpid);
            n
        })
    }
}

#[async_trait]
impl WriteEx for Substream {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.as_mut().expect("already closed?").write2(buf).await.map(|n| {
            self.metric.log_sent_msg(n);
            self.metric.log_sent_stream(self.protocol(), n, &self.info.ci.rpid);
            n
        })
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        self.inner.as_mut().expect("already closed?").flush2().await
    }

    // try to send a CloseStream command to Swarm, then close inner stream
    async fn close2(&mut self) -> Result<(), io::Error> {
        let inner = self.inner.take();
        if let Some(mut inner) = inner {
            // to ask Swarm to remove myself
            let cid = self.cid();
            let sid = StreamId(inner.id());
            let _ = self.ctrl.send(SwarmControlCmd::CloseStream(cid, sid)).await;
            inner.close2().await
        } else {
            Ok(())
        }
    }
}

/// SubstreamView represents the basic information of a substream.
#[derive(Debug, Clone)]
pub struct SubstreamView {
    /// The connection id of the substream.
    pub cid: ConnectionId,
    /// The id of the substream.
    pub id: StreamId,
    /// The protocol of the sub stream.
    pub protocol: ProtocolId,
    /// The direction of the sub stream.
    pub dir: Direction,
}

impl fmt::Display for SubstreamView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Sid({}) {} {}", self.cid, self.id.0, self.dir, self.protocol)
    }
}
