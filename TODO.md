
As v0.1.0 is released, there are still big gaps between `libp2p-rs` and `rust-libp2p`. We'd like to close a bit in near future. Here is a TODO list, categorized per component. Items with higher priority are marked in **BOLD**.


# Core

- **TCP reuse port**: dialing reuse the port that TCP is listening on
- **Transport Upgrade async-post processing**: post processing accept() and protocol upgrading in parallel
- **Metrics**: bandwidth metric counters and report

# Swarm

- **Swarm dialer**: dialing multiple Multiaddr in parallel
- **Event Bus**: A pub/sub event subscription system
- **PeerStore serialization**: serialization/deserialization methods for peer IDs
- Observed address change: handling the observed address changes
- Swarm filters: Multiaddr, PeerId white/black list

# Security Layer

- **Noise**: Noise implementation 

# Transport

- Quicc

# Routing

- **Pubsub**: flood/random/gossip
- KAD/DHT
- mDNS

# Misc.

- NAT port mapping: uPnP or NAT-PMP

