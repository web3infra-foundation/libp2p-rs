# libp2prs-floodsub

> the baseline flooding protocol

This is the canonical pubsub implementation for [libp2p-rs](https://github.com/netwarps/libp2p-rs).

## Usage
#### step1: create floodsub and get handler
```cpp
    let floodsub = FloodSub::new(FloodsubConfig::new(local_peer_id));
    let handler = floodsub.handler();
```
#### step2: register handler to swarm
```cpp
    let swarm = Swarm::new(keys.public())
        .with_transport(Box::new(tu))
        .with_protocol(Box::new(handler))
        .with_ping(PingConfig::new().with_unsolicited(true).with_interval(Duration::from_secs(1)))
        .with_identify(IdentifyConfig::new(false));
```
#### step3: get floodsub control and then start with swarm control
```cpp
    let floodsub_control = floodsub.control();
    floodsub.start(swarm.control());
```
#### step4: start swarm
```cpp
    // listen on
    swarm.listen_on(vec![listen_addr]).unwrap();
    // start swarm
    swarm.start();
    // new connection
    swarm_control.new_connection(remote_peer_id).await.unwrap();
```
#### step5: publish/subscribe/ls/getPeers  
**subscribe**
```cpp
    task::spawn(async move {
        let sub = control.subscribe(b"test").await;
        if let Some(mut sub) = sub {
            loop {
                if let Some(msg) = sub.ch.next().await { log::info!("recived: {:?}", msg.data) }
            }
        }
    });
```
**publish**
```cpp
    floodsub_control.publish(Topic::new(b"test"), msg).await;
```
**ls**
```cpp
    floodsub_control.ls().await;
```
**getPeers**
```cpp
    floodsub_control.get_peers(Topic::new(b"test"));
```
### TODO list:
- config item: sign strict  
- filter repetitive message to prevent over flood  
- blacklist