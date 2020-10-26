

# Overview

[libp2p](https://libp2p.io) is a collection of peer-to-peer networking protocols. This repository provides a Rust implementation of libp2p basic functionality. 



# Components


## Multiaddr

Multiaddr is a standard way to represent addresses that:

- Support any standard network protocols.
- Self-describe (include protocols).
- Have a binary packed format.
- Have a nice string representation.
 
The Multiaddr crate is copied from ParityTech's Multiaddr crate, with slight modifications.


## Transport

Transport represents the network transport layer. It provides connection-oriented communication between two peers through ordered streams of data (i.e. connections). In addtion to the concrete implementations of transport, wrapper transports could be layered on top of an inner transport to provide more functionality, e.g. DNS resolve, timeout, etc.

Two concrete transports:
- TCP
- Websocket

Wrpper transports:
- DNS
- timeout
- Protector: transport with the private network support

### Transport Trait

The trait defines the behaviour of a transport:

- Incomming connection: IListener, the trait object created by transport, can be used to make incomming connections.
- Outgoing connection: Transport::dial can be used to make outgoing connections. 

Note: Transport trait is an async trait.


## Stream Muxing

Stream muxing is very important for libp2p to multiplex the underlying connection (TCP or as such), from a single connection-oriented stream to multiple logigcal sub-streams. 

Two stream muxing implementations are provided so far:

- Yamux (Yet another Multiplexer) : relies on an underlying connection to provide reliability and ordering, such as TCP or Unix domain sockets, and provides stream-oriented multiplexing. 
- Mplex: 


## Security Stream

Security stream provides the secure session over the underlying I/O connection. A handshake is used to setup the communication. Typicaly, a TCP connection created by TCP transport could be upgraded to a secure stream, and after that, the new secure connection will be established using the encryption parameters negotiated by the handshake procedure.

Three security stream implementations are provided:

- Secio: A simple TLS-like security stream implementation.
- Plaintext: mainly for test purpose, no actuall encryption will be done with PlainText, but the PubKey will be exchanged by communication peers as required by the protocol.
- Noise:

## Private Network

Private network offers to setup a PSK based private network using `libp2p-rs`. The node configured with Private network enabled can only communicate with the other node with the same PSK. Basically Private network is implemented as a wrapper transport by adding an encryption/decryption layer on top of the inner tranport. Currently XSalsa20 is used by the private network to provide encryption/decryption capability.

## Upgrader

Upgrader is used to upgrade a connection to use a specific protocol, e.g., from a Tcp stream to a secure stream with SecIo. 

## Multistream Select

Multistream Select is a friendly protocol negotiation. It enables a multicodec to be negotiated between two entities. Details please check [here](https://github.com/multiformats/multistream-select).

## Transport Upgrade

A special transport can be used to upgrade a regular transport to a new one, which is using a upgraded connection with Security Stream and Stream muxing. As a result, Transport Upgrade will estanblish incoming/outgoing connections as secured and multiplxiable stream muxers，on which the logical sub-streams can be build.

It is the essential compoenent of libp2p network layer, and used by Swarm directly. In our implemetation, TransportUpgrade will generate IStreamMuxer as the output, which is the trait object of StreamMuxer. Also TransportUpgrade can be made into trait object as well, so that in Swarm, we can use transport trait object to construct tranports. By using trait object, we remove the generic type, which makes the code quite concise and straitforward. 

                                                                      
## Swarm

Swarm provides the interface to p2p network, implements protocols or provides services. It handles requests like a Server, and issues requests like a Client. Not like `go-libp2p`, in which Swarm is only the low-level access to p2p network, kind of invisible to most applications, Swarm in libp2p-rs is somewhat equivalent to basic-host in `go-libp2p`, which not only manages the connections and sub-streams, but also upgrades the raw sub-stream to a protocol binding sub-stream. Therefore, Swarm APIs is right place to start your own applications with `libp2p-rs`.

More detailes about Swarm please check `docs/swarm.md`.


 

