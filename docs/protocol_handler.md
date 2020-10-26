
# Protocol Handler



A handler for a set of protocols used on a connection with a remote.

This trait should be implemented for a type that maintains the server side state for the execution of a specific protocol.

Trait defined as below:

```no_run
#[async_trait]
pub trait ProtocolHandler: UpgradeInfo {
    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start handling the inbound. Swarm will start invoking this method
    /// in a newly spawned task.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    async fn handle(&mut self, stream: Substream, info: <Self as UpgradeInfo>::Info) -> Result<(), SwarmError>;
    /// This is to provide a clone method for the trait object.
    fn box_clone(&self) -> IProtocolHandler;
}

pub type IProtocolHandler = Box<dyn ProtocolHandler<Info = ProtocolId> + Send + Sync>;
```

> **Note**:: ProtocolHandler is an async trait and can be made into a trait object.


## UpgradeInfo

The trait ProtocolHandler derives from `UpgradeInfo`, which provides a list of protocols that are supported, e.g. '/foo/1.0.0' and '/foo/2.0.0'.


## Handling a protocol

Communication with a remote over a set of protocols is initiated in one of two ways:

  - Dialing by initiating a new outbound substream. In order to do so, `Swarm::control::new_stream()` must be invoked with the specified protocols to create a sub-stream. A protocol negotiation procedure will done for the protocols, in which one might be finally selected. Upon success, a `Swarm::Substream` will be returned by `Swarm::control::new_stream()`, and the protocol will be then handled by the owner of the Substream. 

  - Listening by accepting a new inbound substream. When a new inbound substream is created on a connection, `Swarm::muxer` is called to negotiate the protocol(s). Upon success, `ProtocolHandler::handle` is called with the final output of the upgrade.

## Adding protocol handlers to Swarm

In general, multiple protocol handlers should be made into trait objects and then added to `Swarm::muxer`.

```no_run
    /// Creates Swarm with protocol handler.
    pub fn with_protocol(mut self, p: IProtocolHandler) -> Self {
        self.muxer.add_protocol_handler(p);
        self
    }
```


