
As compared with `rust-libp2p`, we have managed to close many gaps in release v0.2.0, since Kad-DHT is introduced and swarm is improved. However, there are still a long to-do list for us.    

## General

- **Tokio runtime support**

## Core

- **TCP reuse port**: dialing reuse the port that TCP is listening on
- ~~**Transport Upgrade async-post processing**: post processing accept() and protocol upgrading in parallel~~
- ~~**Metrics**: bandwidth metric counters and report~~
- ReadEx/WriteEx/SplitEx removal: unfortunately they are proved to be a failure  

## Swarm

- ~~**Swarm dialer**: dialing multiple Multiaddr in parallel~~
- **Event Bus**: A pub/sub event subscription system: probably not needed any more
- ~~**PeerStore serialization**: serialization/deserialization methods for peer IDs~~
- ~~Observed address change: handling the observed address changes~~
- Swarm identify delta protocol, and certificated peer record
- Swarm filters: Multiaddr, PeerId white/black list


## Security Layer

- ~~**Noise**: Noise implementation~~ 
- TLS

## Transport

- Quicc

## Routing

- **Pubsub**: ~~flood~~/random/gossip
- ~~KAD/DHT~~
- ~~mDNS~~

## Misc.

- NAT port mapping: uPnP or NAT-PMP

