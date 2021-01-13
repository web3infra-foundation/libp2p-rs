# Alternative repository for work on libp2p

<a href="http://libp2p.io/"><img src="https://img.shields.io/badge/project-libp2p-yellow.svg?style=flat-square" /></a>

![Continuous integration](https://github.com/netwarps/libp2p-rs/workflows/Continuous%20integration/badge.svg?branch=master)

This repository is an alternative implementation in `Rust` of the [libp2p](https://libp2p.io) spec. Not like `rust-libp2p`, `libp2p-rs` is written with async/await syntax, and driven by async-std. Even though, many codes are borrowed from `rust-libp2p` and some from `go-libp2p`. We are trying to keep compatible with the two implementations, but it is unfortunately not guaranteed.

## Documentations

How to use the library?

As mentioned above, the API is completely different from `rust-libp2p`. There is no such thing as 'NetworkBehaviour' in `libp2p-rs` at all. Instead, you should build the Swarm with the transports you like to use, then you have to create a Swarm::Control from it. The Swarm::Control is exposing all Swarm APIs which can be used to manipulate the Swarm - open/read/write/close streams and even more. This is quite similar as the BasicHost in `go-libp2p`. As for Kad-DHT, similarly you should get the Kad::Control for the same reason. Furthermore, you can combine the Swarm with Kad, after that you have the RoutedHost, which has a routing functionality over the BasicHost.

It is strongly recommended to check the docs and sample code in details:

- API Documentations can be found: https://docs.rs/libp2p-rs
- Design documentation can be found in `docs`

Code examples:

- Details about how to write your code can be found in `examples`
    + swarm_simple demonstrates how to build transport and create sub-stream for communication
    + kad_simple demonstrates how to run a Kad-DHT server. In this example, the interactive shell is integrated for debugging/observing Kad-DHT internal data structures
    + ... 

## Releases

NOTE: The master branch is now an active development branch (starting with v0.1.0), which means breaking changes could be made at any time.  
