# libp2prs-infoserver

> A visual interface about metric.

This is a web api-server that observe swarm's metric.

## Usage
```
    let keypair = Keypair::generate_ed25519_fixed();
    let swarm = Swarm::new(keypair.public());
    let control = swarm.control();
    
    let monitor = InfoServer::new(control);
    monitor.start("127.0.0.1:8999".to_string());
```