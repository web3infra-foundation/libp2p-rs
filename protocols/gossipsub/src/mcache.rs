// Copyright 2020 Sigma Prime Pty Ltd.
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

use crate::topic::TopicHash;
use crate::types::{MessageId, RawGossipsubMessage};
use libp2prs_core::PeerId;
use log::debug;
use std::fmt::Debug;
use std::{collections::HashMap, fmt};

/// CacheEntry stored in the history.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheEntry {
    mid: MessageId,
    topic: TopicHash,
}

/// MessageCache struct holding history of messages.
#[derive(Clone)]
pub struct MessageCache {
    msgs: HashMap<MessageId, RawGossipsubMessage>,
    /// For every message and peer the number of times this peer asked for the message
    iwant_counts: HashMap<MessageId, HashMap<PeerId, u32>>,
    history: Vec<Vec<CacheEntry>>,
    /// The number of indices in the cache history used for gossipping. That means that a message
    /// won't get gossipped anymore when shift got called `gossip` many times after inserting the
    /// message in the cache.
    gossip: usize,
}

impl fmt::Debug for MessageCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageCache")
            .field("msgs", &self.msgs)
            .field("history", &self.history)
            .field("gossip", &self.gossip)
            .finish()
    }
}

/// Implementation of the MessageCache.
impl MessageCache {
    pub fn new(gossip: usize, history_capacity: usize) -> Self {
        MessageCache {
            gossip,
            msgs: HashMap::default(),
            iwant_counts: HashMap::default(),
            history: vec![Vec::new(); history_capacity],
        }
    }

    /// Put a message into the memory cache.
    ///
    /// Returns the message if it already exists.
    pub fn put(&mut self, message_id: &MessageId, msg: RawGossipsubMessage) -> Option<RawGossipsubMessage> {
        debug!("Put message {:?} in mcache", message_id);
        let cache_entry = CacheEntry {
            mid: message_id.clone(),
            topic: msg.topic.clone(),
        };

        let seen_message = self.msgs.insert(message_id.clone(), msg);
        if seen_message.is_none() {
            // Don't add duplicate entries to the cache.
            self.history[0].push(cache_entry);
        }
        seen_message
    }

    /// Get a message with `message_id`
    #[cfg(test)]
    pub fn get(&self, message_id: &MessageId) -> Option<&RawGossipsubMessage> {
        self.msgs.get(message_id)
    }

    /// Increases the iwant count for the given message by one and returns the message together
    /// with the iwant if the message exists.
    pub fn get_with_iwant_counts(&mut self, message_id: &MessageId, peer: &PeerId) -> Option<(&RawGossipsubMessage, u32)> {
        let iwant_counts = &mut self.iwant_counts;
        self.msgs.get(message_id).and_then(|message| {
            if !message.validated {
                None
            } else {
                Some((message, {
                    let count = iwant_counts.entry(message_id.clone()).or_default().entry(*peer).or_default();
                    *count += 1;
                    *count
                }))
            }
        })
    }

    /// Gets a message with [`MessageId`] and tags it as validated.
    pub fn validate(&mut self, message_id: &MessageId) -> Option<&RawGossipsubMessage> {
        self.msgs.get_mut(message_id).map(|message| {
            message.validated = true;
            &*message
        })
    }

    /// Get a list of [`MessageId`]s for a given topic.
    pub fn get_gossip_message_ids(&self, topic: &TopicHash) -> Vec<MessageId> {
        self.history[..self.gossip].iter().fold(vec![], |mut current_entries, entries| {
            // search for entries with desired topic
            let mut found_entries: Vec<MessageId> = entries
                .iter()
                .filter_map(|entry| {
                    if &entry.topic == topic {
                        let mid = &entry.mid;
                        // Only gossip validated messages
                        if let Some(true) = self.msgs.get(mid).map(|msg| msg.validated) {
                            Some(mid.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            // generate the list
            current_entries.append(&mut found_entries);
            current_entries
        })
    }

    /// Shift the history array down one and delete messages associated with the
    /// last entry.
    pub fn shift(&mut self) {
        for entry in self.history.pop().expect("history is always > 1") {
            if let Some(msg) = self.msgs.remove(&entry.mid) {
                if !msg.validated {
                    // If GossipsubConfig::validate_messages is true, the implementing
                    // application has to ensure that Gossipsub::validate_message gets called for
                    // each received message within the cache timeout time."
                    debug!(
                        "The message with id {} got removed from the cache without being validated.",
                        &entry.mid
                    );
                }
            }
            debug!("Remove message from the cache: {}", &entry.mid);

            self.iwant_counts.remove(&entry.mid);
        }

        // Insert an empty vec in position 0
        self.history.insert(0, Vec::new());
    }

    /// Removes a message from the cache and returns it if existent
    pub fn remove(&mut self, message_id: &MessageId) -> Option<RawGossipsubMessage> {
        //We only remove the message from msgs and iwant_count and keep the message_id in the
        // history vector. Zhe id in the history vector will simply be ignored on popping.

        self.iwant_counts.remove(message_id);
        self.msgs.remove(message_id)
    }
}

// message_id
