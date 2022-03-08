// Copyright 2020 Sigma Prime Pty Ltd.
// Copyright 2021 Netwarps Ltd.
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
pub mod error;
pub mod protocol;
pub mod subscription;

mod backoff;
mod config;
mod gossip_promises;
pub mod gossipsub;
// mod handler;
mod mcache;
mod peer_score;
pub mod subscription_filter;
pub mod time_cache;
pub mod topic;
mod transform;
mod types;

#[cfg(test)]
extern crate derive_builder;

#[allow(unused_mut)]
#[allow(clippy::let_and_return)]
pub mod cli;
mod rpc_proto;

// pub use self::behaviour::{Gossipsub, GossipsubEvent, MessageAuthenticity};
pub use self::transform::{DataTransform, IdentityTransform};

pub use self::config::{GossipsubConfig, GossipsubConfigBuilder, ValidationMode};
pub use self::peer_score::{
    score_parameter_decay, score_parameter_decay_with_base, PeerScoreParams, PeerScoreThresholds, TopicScoreParams,
};
pub use self::topic::{Hasher, Topic, TopicHash};
pub use self::types::{FastMessageId, GossipsubMessage, GossipsubRpc, MessageAcceptance, MessageId, RawGossipsubMessage};
use futures::channel::{mpsc, oneshot};
use std::fmt::{Display, Result};
use std::io;
// pub type IdentTopic = Topic<self::topic::IdentityHash>;
// pub type Sha256Topic = Topic<self::topic::Sha256Hash>;

#[derive(Debug)]
pub enum GossipsubError {
    Io(io::Error),
    Closed,
}

impl std::error::Error for GossipsubError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GossipsubError::Io(err) => Some(err),
            GossipsubError::Closed => None,
        }
    }
}

impl Display for GossipsubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result {
        match self {
            GossipsubError::Io(e) => write!(f, "i/o error: {}", e),
            GossipsubError::Closed => f.write_str("Gossipsub protocol is closed"),
        }
    }
}

impl From<io::Error> for GossipsubError {
    fn from(e: io::Error) -> Self {
        GossipsubError::Io(e)
    }
}

impl From<mpsc::SendError> for GossipsubError {
    fn from(_: mpsc::SendError) -> Self {
        GossipsubError::Closed
    }
}

impl From<oneshot::Canceled> for GossipsubError {
    fn from(_: oneshot::Canceled) -> Self {
        GossipsubError::Closed
    }
}
