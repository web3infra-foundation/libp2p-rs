# Alternative repository for work on libp2p

<a href="http://libp2p.io/"><img src="https://img.shields.io/badge/project-libp2p-yellow.svg?style=flat-square" /></a>

[![Build Status](https://travis-ci.com/netwarps/libp2p-rs.svg?token=tEt4wqXiQg5sktaf43jn&branch=master)](https://travis-ci.com/netwarps/libp2p-rs)


This repository is an alternative implementationof in `Rust` of the [libp2p](https://libp2p.io) spec. Not like `rust-libp2p`, `libp2p-rs` is written with async/await syntax, and driven by async-std. Even though, many codes are borrowed from `rust-libp2p` and some from `go-libp2p`. We are trying to keep compatible with the two implementations, but it is unfortunately not guaranteed.

## Documentations

How to use the library?

- API Documentation can be found: https://docs.rs/libp2p-rs
- Design documentation can be found in `docs`

Code examples:

- More details about how to write your code can be found in `examples`


## Limitations

As for the first stage, we'd like to limit our development scope to deliver the basic functionality equivalent to the basic-host in `go-libp2p`. There is a lone term plan to make a full package which includes the `routing` protocols as `go-libp2p` does. Therefore, the first release will not include any KAD-DHT, mDns and so on. Contributions are welcome to complete the `libp2p-rs` as a full functional libp2p package.     


## Releases

NOTE: The master branch is now an active development branch (starting with v0.1.0), which means breaking changes could be made at any time.  