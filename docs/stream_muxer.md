
Muxing is the process of splitting a connection into multiple substreams.


## StreamMuxer Trait

StreamMuxer Trait is an async trait, used to manipulate substreams over the underlying connection. An implementation of `StreamMuxer` has ownership of a connection, lets you open and close substreams, and read/write data on open substreams.

Each substream of a connection is an isolated stream of data. All the substreams are muxed together so that the data read from or written to each substream doesn't influence the other substreams.

In the context of libp2p, each substream can use a different protocol. Contrary to opening a connection, opening a substream is almost free in terms of resources. This means that you
 shouldn't hesitate to rapidly open and close substreams, and to design protocols that don't require maintaining long-lived channels of communication.
 
```no_run
#[async_trait]
pub trait StreamMuxer {
    /// Opens a new outgoing substream.
    async fn open_stream(&mut self) -> Result<IReadWrite, TransportError>;
    /// Accepts a new incoming substream.
    async fn accept_stream(&mut self) -> Result<IReadWrite, TransportError>;
    /// Closes the stream muxer, the task of stream muxer will then exit.
    async fn close(&mut self) -> Result<(), TransportError>;
    /// Returns a Future which represents the main loop of the stream muxer.
    fn task(&mut self) -> Option<BoxFuture<'static, ()>>;
    /// Returns the cloned Trait object.
    fn box_clone(&self) -> IStreamMuxer;
}

```
## Implementing a muxing protocol

In order to implement a muxing protocol, create an object that implements the `UpgradeInfo` and `Upgrader` traits. See the `upgrader` module for more information. The `Output` associated type of the `Upgrader` traits should be an object that implements the `StreamMuxer` trait.

The upgrade process will take ownership of the connection, which makes it possible for the implementation of `StreamMuxer` to control everything that happens on the wire.

## Implementations

There are two StreamMuxer implementations so far.

### Yamux


### Mplex
