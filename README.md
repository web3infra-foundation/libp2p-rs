# libp2p-rs

[![Build Status](https://travis-ci.com/netwarps/libp2p-rs.svg?token=tEt4wqXiQg5sktaf43jn&branch=master)](https://travis-ci.com/netwarps/libp2p-rs)

## Overview

This is an alternitive of libp2p implementation with rust. It is not intented to replace `rust-libp2p` but to provide a different approach, simply because we believe that `rust-libp2p` is somewhat complicated for developers to understand. In this implementation we tend to write code in a more modern way of using `await` as much as possible, to simplify the code by removing the the annoying `poll` and its state machine handling. 

## Architecture

1. Data stream transmission

```rust
+----+      +----------------+      +-----------+      +-------------+      +----------+      +------+
|user| <--> | custom streams | <--> |Yamux frame| <--> |Secure stream| <--> |TCP stream| <--> |remote|
+----+      +----------------+      +-----------+      +-------------+      +----------+      +------+
```

2. Code implementation

All data is passed through the futures channel, `yamux` splits the actual tcp stream into multiple substreams,
and the service layer wraps the yamux substream into a protocol stream.

At the same time, support for other protocol(such as websocket) is also planned, but will delay a lot.

> Note: we try to keep the compatibility with `rust-libp2p`.

## Status

The API of this project is basically usable. However we still need more tests. PR is welcome.

## Usage

### From cargo

```toml
[dependencies]
libp2p-rs = { version = "0.1", features = ["default"] }
```

### Example

1. Clone

```bash
$ git clone https://github.com/pldteam/libp2p-rs.git
```

2. On one terminal:

Listen on 127.0.0.1:1337
```bash
$ RUST_LOG=debug cargo run --example secio_simple -- server
```

3. On another terminal:

```bash
$ RUST_LOG=debug cargo run --example secio_simple
```

4. Now you can see some data interaction information on the terminal.



## Why?

Because when I use `rust-libp2p`, I have encountered some difficult problems,
and it is difficult to locate whether it is my problem or the library itself,
it is better to implement one myself.
