# Upgrade process
An upgrade is performed in two steps:
- A protocol negotiation step. The `UpgradeInfo::protocol_info` method is called to determine
 which protocols are supported by the trait implementation. The `multistream-select` protocol
is used in order to agree on which protocol to use amongst the ones supported.

- A handshake. After a successful negotiation, the `Upgrader::upgrade_inbound` or
`Upgrader::upgrade_outbound` method is called. This method will return a upgraded
'Connection'. This handshake is considered mandatory, however in practice it is
possible for the trait implementation to return a dummy `Connection` that doesn't perform any
action and immediately succeeds.

After an upgrade is successful, an object of type `Upgrader::Output` is returned.
The actual object depends on the implementation and there is no constraint on the traits that
it should implement, however it is expected that it can be used by the user to control the
behaviour of the protocol.