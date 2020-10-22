### Known issues

The `IListener` of Transport Upgrade should handle the incomming connections and the connnection upgrade in a asynchronous post-processing way, so that the upgrade procedure wouldn't block the `IListener` from producing the next incoming connection, hence further connection setup can be proceeded asynchronously. 

Of course it can be done if we start a task to proceed connection upgrade asynchronously, but it requires introducing async-std async runtime into the libp2p core, which we'd like to not to. An alternative approach is to implement a futures::Stream by proceding IListener::accept and connection upgrade in parallel. To be done...