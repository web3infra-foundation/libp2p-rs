# p2p chat app with libp2p


## Intro
This program demonstrates a simple p2p chat application. It can work between two peers if
1. Both have a private IP address (same network).
2. At least one of them has a public IP address.

Assume if 'A' and 'B' are on different networks host 'A' may or may not have a public IP address but host 'B' has one.

Node A Viedo
[![asciicast](https://asciinema.org/a/0j1M9VBmuHJ94KdleU2r3Tb1g.svg)](https://asciinema.org/a/0j1M9VBmuHJ94KdleU2r3Tb1g)

Node B Viedo
[![asciicast](https://asciinema.org/a/cY5VFxZGgmOq6Z021UGAomUrp.svg)](https://asciinema.org/a/cY5VFxZGgmOq6Z021UGAomUrp)

## Clone code
```
git clone https://github.com/netwarps/libp2p-rs.git
```
## Rust server and Rust client

```
 cd libp2p-rs
```

On node 'B'.
```
 RUST_LOG=info cargo run --example chat server -s 8086
> hi (received messages in green colour)
> hello (sent messages in white colour)
```

On node 'A'. Replace 127.0.0.1 with <PUBLIC_IP> if node 'B' has one.
```
 RUST_LOG=info cargo run --example chat client -d /ip4/127.0.0.1/tcp/8086/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN 
> hi (sent messages in white colour)
> hello (received messages in green colour)
```


## Go server and Go client


run the following:

```
 cd libp2p-rs/examples/chat/go
 make deps
 go build
```

On node 'B'.

```
 ./chat -sp 8086
```

On node 'A'.  

```
 ./chat -d /ip4/127.0.0.1/tcp/8086/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN
```


## Rust server and Go client

On node 'B'.
```
 cd libp2p-rs/
 RUST_LOG=info cargo run --example chat server -s 8086
```

On node 'A'.
```
 cd examples/chat/go
 ./chat -d /ip4/127.0.0.1/tcp/8086/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN
```

##  Go server and Rust client

On node 'B'.
```
 cd examples/chat/go
 ./chat -sp 8086
```

On node 'A'.
```
 cd libp2p-rs/
 RUST_LOG=info cargo run --example chat client -d /ip4/127.0.0.1/tcp/8086/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN 
```