# libp2prs-kad


Comparison of kad implementations between rust-libp2p and go-libp2p:

## Kbuckets, AKA. routing table

### go

+ buckets are expanded on demand
+ no multiaddr, depends on peerstore  
+ insertion: 
    * initialization, to fill kbuckets with all connected peers
    * fix low peers
    * when a remote peer is sending RPC to us
    * when we iteratively query a remote peer successfully  

    // TryAddPeer tries to add a peer to the Routing table.
    // If the peer ALREADY exists in the Routing Table and has been queried before, this call is a no-op.
    // If the peer ALREADY exists in the Routing Table but hasn't been queried before, we set it's LastUsefulAt value to
    // the current time. This needs to done because we don't mark peers as "Useful"(by setting the LastUsefulAt value)
    // when we first connect to them.
    //
    // If the peer is a queryPeer i.e. we queried it or it queried us, we set the LastSuccessfulOutboundQuery to the current time.
    // If the peer is just a peer that we connect to/it connected to us without any DHT query, we consider it as having
    // no LastSuccessfulOutboundQuery.
    //
    //
    // If the logical bucket to which the peer belongs is full and it's not the last bucket, we try to replace an existing peer
    // whose LastSuccessfulOutboundQuery is above the maximum allowed threshold in that bucket with the new peer.
    // If no such peer exists in that bucket, we do NOT add the peer to the Routing Table and return error "ErrPeerRejectedNoCapacity".
    
    // It returns a boolean value set to true if the peer was newly added to the Routing Table, false otherwise.
    // It also returns any error that occurred while adding the peer to the Routing Table. If the error is not nil,
    // the boolean value will ALWAYS be false i.e. the peer wont be added to the Routing Table it it's not already there.
    //
    // A return value of false with error=nil indicates that the peer ALREADY exists in the Routing Table.
+ deletion
    * when a remote peer is found not responding to an outbound query
    * refresh manager: remote peer can not be connected
    * handlePeerChangeEvent: remote is deemed as not qualified as a Kad peer 

### rust

+ all buckets are initialized in the beginning
+ multiaddr embedded in kbuckets
+ insertion: when Kad protocol is confirmed with remote peer -> inboung/outbound protocol upgrade done
    * insert if there is room, pending if there is not
        + KademliaBucketInserts decide how to insert: OnConnected or Manual
    * update the addresses and status if the peer is already or pending in RT
+ deletion: manual only
+ update: when peer is found disconnected
    * always update the addresses and status    
+ NoeStatus: impact how to insert a new peer into RT    
        

  