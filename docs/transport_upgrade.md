
# Transport Upgrade

`TransportUpgrade` is a `Transport` that wraps another `Transport` and adds upgrade capabilities to all inbound and outbound connection attempts. As shown below, `TransportUpgrade` consists of two parts:

- Inner Transport
- TSec: used to upgrade from a regular connection to a secure one
- TMux: used to upgrade from a secure connection to a stream muxer which can be used to open sub-streams

```no_run
pub struct TransportUpgrade<InnerTrans, TMux, TSec> {
    inner: InnerTrans,
    mux: Multistream<TMux>,
    sec: Multistream<TSec>,
}
```

> Note: it implied that TSec and TMux are 'must to have' if constructing a `TransportUpgrade`. Actually they are mandatory and must be in order.

### TSec and TMux 

These two generic types represent the upgrading procedures for Security and Stream Muxing respectively.

TSec is a type implementing `Upgrader`. It takes the inner transport's output and upgrade it to a new connection supporting `SecureInfo`, ReadEx and WriteEx.

```no_run
    TSec: Upgrader<InnerTrans::Output>,
    TSec::Output: SecureInfo + ReadEx + WriteEx + Unpin,
```

TMux is also an `Upgrader`. It takes the TSec::Output as the input and upgrade it to a stream muxer.

```no_run	
    TMux: Upgrader<TSec::Output>,
    TMux::Output: StreamMuxer,
```

> Both TSec and TMux are wrapped and driven by Multistream for protocol negotiation and selection. 


### Known issues

The `IListener` of Transport Upgrade should handle the incomming connections and the connnection upgrade in a asynchronous post-processing way, so that the upgrade procedure wouldn't block the `IListener` from producing the next incoming connection, hence further connection setup can be proceeded asynchronously. 

Of course it can be done if we start a task to proceed connection upgrade asynchronously, but it requires introducing async-std async runtime into the libp2p core, which we'd like to not to. An alternative approach is to implement a futures::Stream by proceding IListener::accept and connection upgrade in parallel. To be done...