# Upgrader

Upgrader Contains everything related to upgrading a connection to use a protocol.

After a connection with a remote has been successfully established. The next step is to *upgrade* this connection to use a protocol.

This is where the `Upgrader` trait comes into play.
The trait is implemented on types that represent a collection of one or more possible protocols for respectively an ingoing or outgoing connection.

> **Note**: Multiple versions of the same protocol are treated as different protocols. For example, `/foo/1.0.0` and `/foo/1.1.0` are totally unrelated as far as upgrading is concerned.


# UpgradeInfo

The trait Upgrader derives from `UpgradeInfo`, which provides a list of protocols that are supported, e.g. '/foo/1.0.0' and '/foo/2.0.0'.

> **Note**: `UpgradeInfo` provides the list of protocols, instead of a single protocol. 

# Upgrade process

An upgrade is performed in two steps:

- A protocol negotiation step. The `UpgradeInfo::protocol_info` method is called to determine which protocols are supported by the trait implementation. The `multistream-select` protocol is used in order to agree on which protocol to use amongst the ones supported.

- A handshake. After a successful negotiation, the `Upgrader::upgrade_inbound` or `Upgrader::upgrade_outbound` method is called. This method will return a upgraded 'Connection'. This handshake is considered mandatory, however in practice it is possible for the trait implementation to return a dummy `Connection` that doesn't perform any action and immediately succeeds.

After an upgrade is successful, an object of type `Upgrader::Output` is returned. The actual object depends on the implementation and there is no constraint on the traits that it should implement, however it is expected that it can be used by the user to control the behaviour of the protocol.
