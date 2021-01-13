## 0.1.0 Initial release (2020.10.26)

### Features

- Transport: Tcp, Dns, Websocket
- Security IO: secio, plaintext, noise
- Stream Muxing: yamux, mplex
- Transport Upgrade: Multistream select, timeout, protector 
- Swarm, with Ping & Identify



## 0.2.0 release (2021.1.12)

### New features and changes

- Protocols
    + **Kad-DHT**:
        * beta-value introduced to better handle iterative query termination
        * configurable timeout for iterative query
        * auto-refresh mechanism to refresh the routing table in a configurable interval
        * health check for any nodes/peers in the routing table whose aliveness is deemed to be outdated
        * event handling for peer identified and local address changed
        * outgoing sub-streams reuse
        * statistics for iterative query - success, failure or timeout
        * debugging shell commands           
    + floodsub: experimental
    + mDns: experimental
- Swarm
    + async post-processing upgrade when accepting new incoming connections**
    + dialer support, dialing multiple addresses in parallel
    + improved identify protocol
    + metric support and many bug fixes
    + notification mechanism for protocol handlers
- Tcp transport: interface address change event
- PeerStore improvement
- Prometheus exporter and Info web server
- An interactive debugging shell, integrated with Swarm and Kad
- Copyright notice updated to conform with MIT license

 