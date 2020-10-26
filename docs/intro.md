


This repository is an alternative implementation of [libp2p](https://libp2p.io) in `Rust`. The details of what libp2p is can be found at [libp2p-spec](https://github.com/libp2p/specs).

## Purpose

It is one of main purposes to build an alternative implementation that is different from `rust-libp2p`, which we believe is too complicated for developers to comprehend to some extent. As we can see, in `rusts-libp2p` the generic types and associated items seem to be abused a bit, which makes the code very hard to understand. Besides, `rust-libp2p` is using `poll` method to write the code, with all kinds of manual futures and streams. All of these are eventually composed and stacked into a huge state machine, which contains a lot of duplicated/similar code snippets. 

We'd like to make some changes.

Actually, we've been expierenced `go-libp2p` for a while and we were kind of impressed by the concise implementation. On the other hand, as for Rust, we believe the network I/O async coding can be done in async/await method, in other words, coroutine method, instead of the traiditional `poll` method, given the fact that the async/await syntax was formally released in the second half of 2019. Therefore, as a basic priciple, we are going to write code using `await` as much as possible, to simplify the code by avoid from `poll` and its state machine handling. In addition to that, we'd like to use `trait object` in many places, which is called 'dynamic dispacting' in most time. Not like the generic parameter based 'static dispacting', this could save us from the generic type flooding in a way. 


## Objective

This repository is not intented to replace `rust-libp2p` but to provide a different approach to `libp2p`. As for the first step, `libp2p-rs` will only have the basic functionality, that is to say, `routing` protocols will not be there. 


## Design Priciples

- No unsafe code
- No manual Future or Stream
- Use Trait Object, dynamic dispatching
- Use futures::mpsc::channel, avoid Mutex<>
- Async Trait, to introduce async fn in Trait
- dependency on async-std for async runtime


## Content

- [`Architecture`](architecture.md)
- [`ReadEx & WriteEx`](readwrite.md)
- [`Transport Upgrade`](treansport_upgrade.md)
- [`Security Stream`](security.md)
- [`Stream Muxer`](stream_muxer.md)
- [`Multistream Select`](multistream.md)
- [`Upgrader`](upgrader.md)
- [`Swarm`](swarm.md)
- [`Protocol Handler`](protocol_handler.md)
