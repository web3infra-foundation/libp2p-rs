PeerStore provides a way to store peer information.

In `go-libp2p-peerstore`, it has two difference implementations: pstoreds and pstoremem. Both of them include three
attributes: AddrBook, ProtoBook, KeyBook.

In `libp2p-rs`, we use a new struct called `PeerRecord` to store all of the attributes.
It may used in concurrency, so outside is designed as a Hashmap that wrapped by `Arc<Mutex>`.

Now let us introduce the structure of `PeerRecord` simply:

`pinned`: This is a symbol for garbage collection. In main loop for `Swarm`, a thread started by `task::spawn` is
responsible for an operation that clean up peerstore every 10 minutes. If `pinned` is true, it means gc will never
recycle this PeerRecord.

`addrs`: A vector saved `AddrBookRecord`. There are three attributes in `AddrBookRecord`: `Multiaddr`, ttl and
the time of initialization(an instant object). When gc is running, if `initial-time.elapsed() > ttl` is true, `AddrBookRecord`
will be deleted. Besides, if `addrs` equals empty, `PeerRecord` will be removed from `Hashmap`.

`key`: For every `peer_id`, there is an unique `public_key` that correspond with it. `public_key` can use method 
`into_peer_id()` to attain `peer_id`, but `peer_id` can't get `public_key` reversely. So we ought to store it.

`protos`: It is used to record all of the protocol types supported by peer. The reason why we use `HashSet` is that
every record can not be replaced when it already exists inside. While we use `Hashmap`, it may cause memory leak or
incorrectly overwriting if re-insert.