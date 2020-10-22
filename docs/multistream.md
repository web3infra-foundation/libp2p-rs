# multistream-select

> Rust implementation of [multistream-select](https://github.com/multiformats/multistream-select)

## Table of Contents

- [Background](#background)
  - [What is multistream-select?](#what-is-multistream-select)
    - [Select a protocol flow](#select-a-protocol-flow)
- [Usage](#usage)
    - [Dialer](#dialer)
    - [Listener](#listener)

## Background

### What is `multistream-select`?

TLDR; multistream-select is protocol multiplexing per connection/stream. [Full spec here](https://github.com/multiformats/multistream-select)

#### Select a protocol flow

The caller will send "interactive" messages, expecting for some acknowledgement from the callee, which will "select" the handler for the desired and supported protocol:

```console
< /multistream/1.0.0  # i speak multistream/1.0.0
> /multistream/1.0.0  # ok, let's speak multistream/1.0.0
> /proto1/0.2.3       # i want to speak proto1/0.2.3
< na                  # proto1/0.2.3 is not available
> /proto1/0.1.9       # What about proto1/0.1.9 ?
< /proto1/0.1.9       # ok let's speak proto1/0.1.9 -- in a sense acts as an ACK
> <proto1-message>
> <proto1-message>
> <proto1-message>
```

This mode also packs a `ls` option, so that the callee can list the protocols it currently supports

## Usage

```rust
let protos = vec!["/proto1/1.9.0", "/proto1/2.0.1"];
let neg = Negotiator::new_with_protocols(protos);
```

### Dialer

```rust
let client = async_std::task::spawn(async move {
    let connec = TcpStream::connect(&listener_addr).await.unwrap();
    let protos = vec!["/proto1/1.9.0", "/proto1/2.0.1"];
    let neg = Negotiator::new_with_protocols(protos);
    let (proto, mut io) = neg.select_one(connec).await.expect("select_one");
    assert_eq!(proto, "/proto1/1.9.0");

    io.write_all(b"ping").await.unwrap();
    io.flush().await.unwrap();

    let mut out = vec![0; 32];
    let n = io.read(&mut out).await.unwrap();
    out.truncate(n);
    assert_eq!(out, b"pong");
});
```

### Listener

```rust
let server = async_std::task::spawn(async move {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connec = listener.accept().await.unwrap().0;
    let protos = vec!["/proto1/1.9.0", "/proto1/2.0.1"];
    let neg = Negotiator::new_with_protocols(protos);
    let (proto, mut io) = neg.negotiate(connec).await.expect("negotiate");
    assert_eq!(proto, "/proto1/1.9.0");

    let mut out = vec![0; 32];
    let n = io.read(&mut out).await.unwrap();
    out.truncate(n);
    assert_eq!(out, b"ping");

    io.write_all(b"pong").await.unwrap();
    io.flush().await.unwrap();
});
```
