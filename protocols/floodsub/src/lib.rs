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

pub mod control;
pub mod floodsub;
pub mod protocol;
mod subscription;

use libp2prs_core::PeerId;
use std::{
    fmt::{Display, Result},
    io,
};

mod rpc_proto {
    include!(concat!(env!("OUT_DIR"), "/floodsub.pb.rs"));
}

const FLOOD_SUB_ID: &[u8] = b"/floodsub/1.0.0";

/// Configuration options for the Floodsub protocol.
#[derive(Clone)]
pub struct FloodsubConfig {
    /// Peer id of the local node. Used for the source of the messages that we publish.
    pub local_peer_id: PeerId,

    /// `true` if messages published by local node should be propagated as messages received from
    /// the network, `false` by default.
    pub subscribe_local_messages: bool,
}

impl FloodsubConfig {
    pub fn new(local_peer_id: PeerId) -> Self {
        Self {
            local_peer_id,
            subscribe_local_messages: false,
        }
    }
}

/// Built topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Topic(String);

impl Topic {
    /// Returns the id of the topic.
    #[inline]
    pub fn id(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == ""
    }

    pub fn new<S>(name: S) -> Topic
    where
        S: Into<String>,
    {
        Topic(name.into())
    }
}

impl From<Topic> for String {
    fn from(topic: Topic) -> String {
        topic.0
    }
}

#[derive(Debug)]
pub enum FloodsubError {
    Io(io::Error),
    Closed,
}

impl Display for FloodsubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result {
        match self {
            FloodsubError::Io(e) => write!(f, "i/o error: {}", e),
            FloodsubError::Closed => f.write_str("floodsub protocol is closed"),
        }
    }
}

impl From<io::Error> for FloodsubError {
    fn from(e: io::Error) -> Self {
        FloodsubError::Io(e)
    }
}
