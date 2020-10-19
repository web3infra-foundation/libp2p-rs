

# Overview

[libp2p](https://libp2p.io) is a collection of peer-to-peer networking protocols. This repository provides a Rust implementation of libp2p basic functionality. 


# Components


## Multiaddr

Multiaddr is a standard way to represent addresses that:

Support any standard network protocols.
Self-describe (include protocols).
Have a binary packed format.
Have a nice string representation.
Encapsulate well.
 
The Multiaddr crate is copied from ParityTech's Multiaddr crate, with slight modifications.


## Transport

Transport represents the network transport layer. It provides connection-oriented communication between two peers through ordered streams of data (i.e. connections).

## Stream Muxing

Yamux (Yet another Multiplexer) is a multiplexing library for Golang. It relies on an underlying connection to provide reliability and ordering, such as TCP or Unix domain sockets, and provides stream-oriented multiplexing. It is inspired by SPDY but is not interoperable with it.

Yamux features include:

Bi-directional streams
Streams can be opened by either client or server
Useful for NAT traversal
Server-side push support
Flow control
Avoid starvation
Back-pressure to prevent overwhelming a receiver
Keep Alives
Enables persistent connections over a load balancer
Efficient
Enables thousands of logical streams with low overhead


## Security Stream

Connections wrapped by secio use secure sessions provided by this package to encrypt all traffic. A TLS-like handshake is used to setup the communication channel.

## Multistream Select

Friendly protocol negotiation. It enables a multicodec to be negotiated between two entities.  [here](https://github.com/multiformats/multistream-select).

                
                                                      
## Swarm

Swarm provides the interface to p2p network, implements protocols or provides services. It handles requests like a Server, and issues requests like a Client. Not like `go-libp2p`, in which Swarm is only the low-level access to p2p network, Swarm in libp2p-rs is somewhat equivalent to basic-host, which not only manages the connections and sub-streams, but also upgrades the raw sub-stream to a protocol binding sub-stream. It means Swarm APIs is right place to start your own applications. As a comparison, Swarm is kind of invisible to most applications.


## Getting started

In general, to use libp2p-rs, you would always create a Swarm object to access the low-level network:

Creating a swarm:



It takes five items to fully construct a swarm, the first is a go context.Context. This controls the lifetime of the swarm, and all swarm processes have their lifespan derived from the given context. You can just use context.Background() if you're not concerned with that.

The next argument is an array of multiaddrs that the swarm will open up listeners for. Once started, the swarm will start accepting and handling incoming connections on every given address. This argument is optional, you can pass nil and the swarm will not listen for any incoming connections (but will still be able to dial out to other peers).

After that, you'll need to give the swarm an identity in the form of a peer.ID. If you're not wanting to enable secio (libp2p's transport layer encryption), then you can pick any string for this value. For example peer.ID("FooBar123") would work. Note that passing a random string ID will result in your node not being able to communicate with other peers that have correctly generated IDs. To see how to generate a proper ID, see the below section on "Identity Generation".

The fourth argument is a peerstore. This is essentially a database that the swarm will use to store peer IDs, addresses, public keys, protocol preferences and more. You can construct one by importing github.com/libp2p/go-libp2p-peerstore and calling peerstore.NewPeerstore().

The final argument is a bandwidth metrics collector, This is used to track incoming and outgoing bandwidth on connections managed by this swarm. It is optional, and passing nil will simply result in no metrics for connections being available.



 