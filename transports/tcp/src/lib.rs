//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! # Usage
//!
//! This crate provides `TcpConfig`
//!
//! The `TcpConfig` struct implements the `Transport` trait of the
//! `core` library. See the documentation of `core` and of libp2p in general to learn how to
//! use the `Transport` trait.

use async_std::net::{TcpListener, TcpStream};
use async_trait::async_trait;
use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::transport::ConnectionInfo;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{TransportError, TransportListener},
    Transport,
};
use log::debug;
use socket2::{Domain, Socket, Type};
use std::{
    convert::TryFrom,
    io,
    iter::{self, FromIterator},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the reactor.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
#[derive(Debug, Clone, Default)]
pub struct TcpConfig {
    /// How long a listener should sleep after receiving an error, before trying again.
    sleep_on_error: Duration,
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig {
            sleep_on_error: Duration::from_millis(100),
            ttl: None,
            nodelay: None,
        }
    }

    /// Sets the TTL to set for opened sockets.
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Sets the `TCP_NODELAY` to set for opened sockets.
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }
}

#[async_trait]
impl Transport for TcpConfig {
    type Output = TcpTransStream;
    type Listener = TcpTransListener;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let socket = if socket_addr.is_ipv4() {
            Socket::new(Domain::ipv4(), Type::stream(), Some(socket2::Protocol::tcp()))?
        } else {
            let s = Socket::new(Domain::ipv6(), Type::stream(), Some(socket2::Protocol::tcp()))?;
            s.set_only_v6(true)?;
            s
        };
        if cfg!(target_family = "unix") {
            socket.set_reuse_address(true)?;
        }
        socket.bind(&socket_addr.into())?;
        socket.listen(1024)?; // we may want to make this configurable

        let listener = <TcpListener>::try_from(socket.into_tcp_listener()).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let local_addr = listener.local_addr()?;
        let port = local_addr.port();

        let ma = ip_to_multiaddr(local_addr.ip(), port);
        debug!("Listening on {:?}", ma);

        // kingwel, don't care about the unspecified address, so far
        // TBD
        /*
                // Determine all our listen addresses which is either a single local IP address
                // or (if a wildcard IP address was used) the addresses of all our interfaces,
                // as reported by `get_if_addrs`.
                let addrs =
                    if socket_addr.ip().is_unspecified() {
                        let addrs = host_addresses(port)?;
                        debug!("Listening on {:?}", addrs.iter().map(|(_, _, ma)| ma).collect::<Vec<_>>());
                        Addresses::Many(addrs)
                    } else {
                        let ma = ip_to_multiaddr(local_addr.ip(), port);
                        debug!("Listening on {:?}", ma);
                        Addresses::One(ma)
                    };

                // Generate `NewAddress` events for each new `Multiaddr`.
                let pending = match addrs {
                    Addresses::One(ref ma) => {
                        let event = ListenerEvent::NewAddress(ma.clone());
                        let mut list = VecDeque::new();
                        list.push_back(Ok(event));
                        list
                    }
                    Addresses::Many(ref aa) => {
                        aa.iter()
                            .map(|(_, _, ma)| ma)
                            .cloned()
                            .map(ListenerEvent::NewAddress)
                            .map(Result::Ok)
                            .collect::<VecDeque<_>>()
                    }
                };
        */
        let listener = TcpTransListener {
            inner: listener,
            pause: None,
            pause_duration: self.sleep_on_error,
            port,
            ma,
            config: self,
        };

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                debug!("Instantly refusing dialing {}, as it is invalid", addr);
                return Err(TransportError::IoError(io::ErrorKind::ConnectionRefused.into()));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        debug!("Dialing {}", addr);

        let stream = TcpStream::connect(&socket_addr).await?;
        apply_config(&self, &stream)?;

        // figure out the local sock address
        let local_addr = stream.local_addr()?;
        let la = ip_to_multiaddr(local_addr.ip(), local_addr.port());
        Ok(TcpTransStream {
            inner: stream,
            la,
            ra: addr,
        })
    }
}

/// Wraps around a `TcpListener`.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
#[derive(Debug)]
pub struct TcpTransListener {
    inner: TcpListener,
    /// The current pause if any.
    pause: Option<Delay>,
    /// How long to pause after an error.
    pause_duration: Duration,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    ma: Multiaddr,
    /// Original configuration.
    config: TcpConfig,
}

#[async_trait]
impl TransportListener for TcpTransListener {
    type Output = TcpTransStream;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let (stream, sock_addr) = self.inner.accept().await?;
        apply_config(&self.config, &stream)?;

        let local_addr = stream.local_addr()?;
        let la = ip_to_multiaddr(local_addr.ip(), local_addr.port());
        let ra = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());
        Ok(TcpTransStream { inner: stream, la, ra })
    }

    fn multi_addr(&self) -> Multiaddr {
        self.ma.clone()
    }
}
/*
/// Stream that listens on an TCP/IP address.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
pub struct TcpListenStream {
    /// The incoming connections.
    stream: TcpListener,
    /// The current pause if any.
    pause: Option<Delay>,
    /// How long to pause after an error.
    pause_duration: Duration,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    addrs: Addresses,
    /// Temporary buffer of listener events.
    pending: Buffer<TcpTransStream>,
    /// Original configuration.
    config: TcpConfig
}

impl TcpListenStream {
    /// Takes ownership of the listener, and returns the next incoming event and the listener.
    async fn next(mut self) -> (Result<ListenerEvent<Ready<Result<TcpTransStream, io::Error>>, io::Error>, io::Error>, Self) {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return (event, self);
            }

            if let Some(pause) = self.pause.take() {
                let _ = pause.await;
            }

            // TODO: do we get the peer_addr at the same time?
            let (sock, _) = match self.stream.accept().await {
                Ok(s) => s,
                Err(e) => {
                    debug!("error accepting incoming connection: {}", e);
                    self.pause = Some(Delay::new(self.pause_duration));
                    return (Ok(ListenerEvent::Error(e)), self);
                }
            };

            let sock_addr = match sock.peer_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    debug!("Failed to get peer address: {:?}", err);
                    continue
                }
            };

            let local_addr = match sock.local_addr() {
                Ok(sock_addr) => {
                    if let Addresses::Many(ref mut addrs) = self.addrs {
                        if let Err(err) = check_for_interface_changes(&sock_addr, self.port, addrs, &mut self.pending) {
                            return (Ok(ListenerEvent::Error(err)), self);
                        }
                    }
                    ip_to_multiaddr(sock_addr.ip(), sock_addr.port())
                }
                Err(err) => {
                    debug!("Failed to get local address of incoming socket: {:?}", err);
                    continue
                }
            };

            let remote_addr = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());

            match apply_config(&self.config, &sock) {
                Ok(()) => {
                    trace!("Incoming connection from {} at {}", remote_addr, local_addr);
                    self.pending.push_back(Ok(ListenerEvent::Upgrade {
                        upgrade: future::ok(TcpTransStream { inner: sock }),
                        local_addr,
                        remote_addr
                    }))
                }
                Err(err) => {
                    debug!("Error upgrading incoming connection from {}: {:?}", remote_addr, err);
                    self.pending.push_back(Ok(ListenerEvent::Upgrade {
                        upgrade: future::err(err),
                        local_addr,
                        remote_addr
                    }))
                }
            }
        }
    }
}
*/
/// Wraps around a `TcpStream` and adds logging for important events.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
#[derive(Debug, Clone)]
pub struct TcpTransStream {
    inner: TcpStream,
    la: Multiaddr,
    ra: Multiaddr,
}

impl ConnectionInfo for TcpTransStream {
    fn local_multiaddr(&self) -> Multiaddr {
        self.la.clone()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.ra.clone()
    }
}

impl Drop for TcpTransStream {
    fn drop(&mut self) {
        if let Ok(addr) = self.inner.peer_addr() {
            debug!("Dropped TCP connection to {:?}", addr);
        } else {
            debug!("Dropped TCP connection to undeterminate peer");
        }
    }
}

/// Applies the socket configuration parameters to a socket.
fn apply_config(config: &TcpConfig, socket: &TcpStream) -> Result<(), io::Error> {
    if let Some(ttl) = config.ttl {
        socket.set_ttl(ttl)?;
    }

    if let Some(nodelay) = config.nodelay {
        socket.set_nodelay(nodelay)?;
    }

    Ok(())
}

impl AsyncRead for TcpTransStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.inner), cx, buf)
    }
}

impl AsyncWrite for TcpTransStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.inner), cx)
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        //return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let proto = match ip {
        IpAddr::V4(ip) => Protocol::Ip4(ip),
        IpAddr::V6(ip) => Protocol::Ip6(ip),
    };
    let it = iter::once(proto).chain(iter::once(Protocol::Tcp(port)));
    Multiaddr::from_iter(it)
}
/*
// Collect all local host addresses and use the provided port number as listen port.
fn host_addresses(port: u16) -> io::Result<Vec<(IpAddr, IpNet, Multiaddr)>> {
    let mut addrs = Vec::new();
    for iface in get_if_addrs()? {
        let ip = iface.ip();
        let ma = ip_to_multiaddr(ip, port);
        let ipn = match iface.addr {
            IfAddr::V4(ip4) => {
                let prefix_len = (!u32::from_be_bytes(ip4.netmask.octets())).leading_zeros();
                let ipnet = Ipv4Net::new(ip4.ip, prefix_len as u8)
                    .expect("prefix_len is the number of bits in a u32, so can not exceed 32");
                IpNet::V4(ipnet)
            }
            IfAddr::V6(ip6) => {
                let prefix_len = (!u128::from_be_bytes(ip6.netmask.octets())).leading_zeros();
                let ipnet = Ipv6Net::new(ip6.ip, prefix_len as u8)
                    .expect("prefix_len is the number of bits in a u128, so can not exceed 128");
                IpNet::V6(ipnet)
            }
        };
        addrs.push((ip, ipn, ma))
    }
    Ok(addrs)
}

/// Listen address information.
#[derive(Debug)]
enum Addresses {
    /// A specific address is used to listen.
    One(Multiaddr),
    /// A set of addresses is used to listen.
    Many(Vec<(IpAddr, IpNet, Multiaddr)>)
}

// If we listen on all interfaces, find out to which interface the given
// socket address belongs. In case we think the address is new, check
// all host interfaces again and report new and expired listen addresses.
fn check_for_interface_changes<T>(
    socket_addr: &SocketAddr,
    listen_port: u16,
    listen_addrs: &mut Vec<(IpAddr, IpNet, Multiaddr)>,
    pending: &mut Buffer<T>
) -> Result<(), io::Error> {
    // Check for exact match:
    if listen_addrs.iter().find(|(ip, ..)| ip == &socket_addr.ip()).is_some() {
        return Ok(())
    }

    // No exact match => check netmask
    if listen_addrs.iter().find(|(_, net, _)| net.contains(&socket_addr.ip())).is_some() {
        return Ok(())
    }

    // The local IP address of this socket is new to us.
    // We check for changes in the set of host addresses and report new
    // and expired addresses.
    //
    // TODO: We do not detect expired addresses unless there is a new address.
    let old_listen_addrs = std::mem::replace(listen_addrs, host_addresses(listen_port)?);

    // Check for addresses no longer in use.
    for (ip, _, ma) in old_listen_addrs.iter() {
        if listen_addrs.iter().find(|(i, ..)| i == ip).is_none() {
            debug!("Expired listen address: {}", ma);
            pending.push_back(Ok(ListenerEvent::AddressExpired(ma.clone())));
        }
    }

    // Check for new addresses.
    for (ip, _, ma) in listen_addrs.iter() {
        if old_listen_addrs.iter().find(|(i, ..)| i == ip).is_none() {
            debug!("New listen address: {}", ma);
            pending.push_back(Ok(ListenerEvent::NewAddress(ma.clone())));
        }
    }

    // We should now be able to find the local address, if not something
    // is seriously wrong and we report an error.
    if listen_addrs.iter()
        .find(|(ip, net, _)| ip == &socket_addr.ip() || net.contains(&socket_addr.ip()))
        .is_none()
    {
        let msg = format!("{} does not match any listen address", socket_addr.ip());
        return Err(io::Error::new(io::ErrorKind::Other, msg))
    }

    Ok(())
}
*/

#[cfg(test)]
mod tests {
    use super::multiaddr_to_socketaddr;
    #[cfg(feature = "async-std")]
    use super::TcpConfig;
    use libp2p_core::multiaddr::Multiaddr;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::Ipv6Addr;

        assert!(multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()).is_err());

        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/255.255.255.255/tcp/8080".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,)),
                8080,
            ))
        );
    }

    #[test]
    #[cfg(feature = "async-std")]
    fn dialer_and_listener_timeout() {
        fn test1(addr: Multiaddr) {
            async_std::task::block_on(async move {
                let mut timeout_listener = TcpConfig::new().timeout(Duration::from_secs(1)).listen_on(addr).unwrap();
                assert!(timeout_listener.accept().await.is_err());
            });
        }

        fn test2(addr: Multiaddr) {
            async_std::task::block_on(async move {
                let tcp = TcpConfig::new().timeout(Duration::from_secs(1));
                assert!(tcp.dial(addr.clone()).await.is_err());
            });
        }

        test1("/ip4/127.0.0.1/tcp/1111".parse().unwrap());
        test2("/ip4/127.0.0.1/tcp/1111".parse().unwrap());
    }

    #[test]
    #[cfg(feature = "async-std")]
    fn communicating_between_dialer_and_listener() {
        fn test(addr: Multiaddr) {
            let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
            let mut ready_tx = Some(ready_tx);

            async_std::task::spawn(async move {
                let mut tcp_listener = TcpConfig::new().listen_on(addr).unwrap();

                ready_tx.take().unwrap().send(tcp_listener.multi_addr()).unwrap();

                loop {
                    let mut socket = tcp_listener.accept().await.unwrap();

                    let mut buf = [0u8; 3];
                    socket.read_exact(&mut buf).await.unwrap();
                    assert_eq!(buf, [1, 2, 3]);
                    socket.write_all(&[4, 5, 6]).await.unwrap();
                }

                /*
                                let mut listener = tcp.listen_on(addr).await.unwrap();

                                loop {
                                    match listener.next().await.unwrap().unwrap() {
                                        ListenerEvent::NewAddress(listen_addr) => {
                                            ready_tx.take().unwrap().send(listen_addr).unwrap();
                                        },
                                        ListenerEvent::Upgrade { upgrade, .. } => {
                                            let mut upgrade = upgrade.await.unwrap();
                                            let mut buf = [0u8; 3];
                                            upgrade.read_exact(&mut buf).await.unwrap();
                                            assert_eq!(buf, [1, 2, 3]);
                                            upgrade.write_all(&[4, 5, 6]).await.unwrap();
                                        },
                                        _ => unreachable!()
                                    }
                                }
                */
            });

            async_std::task::block_on(async move {
                let addr = ready_rx.await.unwrap();
                let tcp = TcpConfig::new();

                // Obtain a future socket through dialing
                let mut socket = tcp.dial(addr.clone()).await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/1110".parse().unwrap());
        test("/ip6/::1/tcp/1110".parse().unwrap());
    }
    /*
        #[test]
        #[cfg(feature = "async-std")]
        fn replace_port_0_in_returned_multiaddr_ipv4() {
            let tcp = TcpConfig::new();

            let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
            assert!(addr.to_string().contains("tcp/0"));

            let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).await.unwrap())
                .next()
                .expect("some event")
                .expect("no error")
                .into_new_address()
                .expect("listen address");

            assert!(!new_addr.to_string().contains("tcp/0"));
        }

        #[test]
        #[cfg(feature = "async-std")]
        fn replace_port_0_in_returned_multiaddr_ipv6() {
            let tcp = TcpConfig::new();

            let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
            assert!(addr.to_string().contains("tcp/0"));

            let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).unwrap())
                .next()
                .expect("some event")
                .expect("no error")
                .into_new_address()
                .expect("listen address");

            assert!(!new_addr.to_string().contains("tcp/0"));
        }
        #[test]
        #[cfg(feature = "async-std")]
        fn larger_addr_denied() {
            let tcp = TcpConfig::new();

            let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
                .parse::<Multiaddr>()
                .unwrap();

            //let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).await.unwrap());
            let ret = async_std::task::block_on(tcp.listen_on(addr)).is_err();
            assert!(!ret);
        }
    */
}
