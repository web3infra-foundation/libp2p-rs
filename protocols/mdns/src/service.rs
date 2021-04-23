// Copyright 2018 Parity Technologies (UK) Ltd.
// Copyright 2020 Netwarps Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use dns_parser::{Packet, RData};
use futures::{
    channel::{mpsc, oneshot},
    select, FutureExt, StreamExt,
};
use futures_timer::Delay;
use lazy_static::lazy_static;
use nohash_hasher::IntMap;
use smallvec::SmallVec;
use std::{
    convert::TryFrom,
    io, iter,
    net::{Ipv4Addr, SocketAddr},
    str,
    time::Duration,
};
use std::{error::Error, fmt};

use libp2prs_core::multiaddr::protocol::Protocol;
use libp2prs_core::translation::address_translation;
use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_runtime::{net::UdpSocket, task};

use crate::control::Control;
use crate::control::RegId;
use crate::dns::build_service_discovery_response;
use crate::{dns, AddrInfo, INotifiee, MdnsConfig, META_QUERY_SERVICE, SERVICE_NAME};

const MDNS_RESPONSE_TTL: std::time::Duration = Duration::from_secs(5 * 60);

lazy_static! {
    static ref IPV4_MDNS_MULTICAST_ADDRESS: SocketAddr = SocketAddr::from((Ipv4Addr::new(224, 0, 0, 251), 5353,));
}

pub enum ControlCommand {
    RegisterNotifee(INotifiee, oneshot::Sender<RegId>),
    UnregisterNotifee(RegId),
}

pub struct MdnsService {
    /// libp2prs-Mdns config.
    config: MdnsConfig,

    /// Used to register and unregister notifiee.
    control_tx: mpsc::Sender<ControlCommand>,
    control_rx: mpsc::Receiver<ControlCommand>,

    /// All registered notifiees.
    notifees: IntMap<RegId, INotifiee>,
}

impl MdnsService {
    /// Create a new 'MdnsService'.
    pub fn new(config: MdnsConfig) -> Self {
        let (control_tx, control_rx) = mpsc::channel(32);

        MdnsService {
            config,
            control_tx,
            control_rx,
            notifees: IntMap::default(),
        }
    }

    /// Get control of mdns service, which can be used to register and unregister notifiee.
    pub fn control(&self) -> Control {
        Control::new(self.control_tx.clone())
    }

    /// start MDNS service.
    pub fn start(self) {
        let mut service = self;
        task::spawn(async move {
            let _ = service.next().await;
        });
    }

    async fn next(&mut self) -> io::Result<Box<dyn Error>> {
        // We have to create udp socket locally instead of member of MdnsService.
        // Otherwise, when we select, compiler will complain about coexist of the immut and mut ref
        let (socket, query_socket) = bind().expect("bind failed");
        let mut mrecv_buffer = [0; 4096];
        let mut urecv_buffer = [0; 4096];

        let mut timer = Box::pin(Delay::new(Duration::from_secs(10)).fuse());
        loop {
            // It is better to pin these futures outside "select" macro,
            // otherwise compiler will give me a confused complain with "recursion limit reached while expanding"
            let mut m_recv = Box::pin(recv(&query_socket, &mut mrecv_buffer).fuse());
            let mut u_recv = Box::pin(recv(&socket, &mut urecv_buffer).fuse());

            select! {
                (data, from) = m_recv => {
                    let packet = MdnsPacket::new_from_bytes(&data, from);
                    self.handle_received_packet(&socket, packet).await;
                },
                (data, from) = u_recv => {
                    let packet = MdnsPacket::new_from_bytes(&data, from);
                    self.handle_received_packet(&socket, packet).await;
                },
                cmd = self.control_rx.next() => {
                    self.on_control_command(cmd);
                },
                _r = timer => {
                    timer = Box::pin(Delay::new(Duration::from_secs(10)).fuse());
                    if !self.config.silent {
                        let query = dns::build_query();
                        query_socket
                            .send_to(&query, *IPV4_MDNS_MULTICAST_ADDRESS)
                            .await
                            .expect("send query failed");
                    }
                }
            }
        }
    }

    async fn handle_received_packet(&mut self, socket: &UdpSocket, packet: Option<MdnsPacket>) {
        if let Some(packet) = packet {
            match packet {
                MdnsPacket::Query(query) => {
                    let raddr = query.remote_addr();
                    log::debug!("Query from {:?}", query.remote_addr());
                    let resp = dns::build_query_response(
                        query.query_id,
                        self.config.local_peer,
                        self.config.listened_addrs.clone().into_iter(),
                        MDNS_RESPONSE_TTL,
                    )
                    .unwrap();

                    socket.send_to(&resp, *raddr).await.expect("send query response failed");
                }
                MdnsPacket::Response(resp) => {
                    // We replace the IP address with the address we observe the
                    // remote as and the address they listen on.
                    let obs_ip = Protocol::from(resp.remote_addr().ip());
                    let obs_port = Protocol::Udp(resp.remote_addr().port());
                    let observed: Multiaddr = iter::once(obs_ip).chain(iter::once(obs_port)).collect();

                    let mut discovered: SmallVec<[_; 4]> = SmallVec::new();
                    for peer in resp.discovered_peers() {
                        if peer.id() == &self.config.local_peer {
                            continue;
                        }

                        // TODO: manage time-to-live of peer
                        let mut addrs: Vec<Multiaddr> = Vec::new();
                        for addr in peer.addresses() {
                            if let Some(new_addr) = address_translation(&addr, &observed) {
                                addrs.push(new_addr.clone())
                            }
                            addrs.push(addr.clone())
                        }

                        for addr in addrs {
                            discovered.push((*peer.id(), addr));
                        }
                    }

                    for (pid, addr) in discovered {
                        let peer = AddrInfo { pid, addrs: vec![addr] };
                        for noti in self.notifees.values_mut() {
                            noti.handle_peer_found(peer.clone());
                        }
                    }
                }
                MdnsPacket::ServiceDiscovery(disc) => {
                    let resp = build_service_discovery_response(disc.query_id(), MDNS_RESPONSE_TTL);

                    socket
                        .send_to(&resp, *IPV4_MDNS_MULTICAST_ADDRESS)
                        .await
                        .expect("send query response failed");
                }
            }
        }
    }

    fn on_control_command(&mut self, cmd: Option<ControlCommand>) {
        match cmd {
            Some(ControlCommand::RegisterNotifee(noti, reply)) => {
                let id = RegId::random();
                self.notifees.insert(id, noti);
                let _ = reply.send(id);
            }
            Some(ControlCommand::UnregisterNotifee(id)) => {
                self.notifees.remove(&id);
            }
            None => {}
        }
    }
}

fn bind() -> io::Result<(UdpSocket, UdpSocket)> {
    let std_socket = {
        #[cfg(unix)]
        fn platform_specific(s: &net2::UdpBuilder) -> io::Result<()> {
            net2::unix::UnixUdpBuilderExt::reuse_port(s, true)?;
            Ok(())
        }
        #[cfg(not(unix))]
        fn platform_specific(_: &net2::UdpBuilder) -> io::Result<()> {
            Ok(())
        }
        let builder = net2::UdpBuilder::new_v4()?;
        builder.reuse_address(true)?;
        platform_specific(&builder)?;
        builder.bind(("0.0.0.0", 5353))?
    };

    let socket = UdpSocket::from(std_socket);

    // Given that we pass an IP address to bind, which does not need to be resolved, we can
    let query_socket = UdpSocket::from(std::net::UdpSocket::bind((Ipv4Addr::from([0u8, 0, 0, 0]), 0u16))?);

    socket.set_multicast_loop_v4(true).expect("couldn't set multicast loop");
    socket.set_multicast_ttl_v4(255).expect("couldn't set multicast ttl");
    // TODO: correct interfaces?
    socket.join_multicast_v4(Ipv4Addr::new(224, 0, 0, 251), Ipv4Addr::UNSPECIFIED)?;

    Ok((socket, query_socket))
}

async fn recv<'a>(socket: &'a UdpSocket, buf: &'a mut [u8]) -> (&'a [u8], SocketAddr) {
    let (len, from) = socket.recv_from(buf).await.expect("recv from failed");
    let (data, _) = buf.split_at(len);
    (data, from)
}

/// A valid mDNS packet received by the service.
#[derive(Debug)]
pub enum MdnsPacket {
    /// A query made by a remote.
    Query(MdnsQuery),
    /// A response sent by a remote in response to one of our queries.
    Response(MdnsResponse),
    /// A request for service discovery.
    ServiceDiscovery(MdnsServiceDiscovery),
}

impl MdnsPacket {
    fn new_from_bytes(buf: &[u8], from: SocketAddr) -> Option<MdnsPacket> {
        match Packet::parse(buf) {
            Ok(packet) => {
                if packet.header.query {
                    if packet.questions.iter().any(|q| q.qname.to_string().as_bytes() == SERVICE_NAME) {
                        let query = MdnsPacket::Query(MdnsQuery {
                            from,
                            query_id: packet.header.id,
                        });
                        Some(query)
                    } else if packet
                        .questions
                        .iter()
                        .any(|q| q.qname.to_string().as_bytes() == META_QUERY_SERVICE)
                    {
                        // TODO: what if multiple questions, one with SERVICE_NAME and one with META_QUERY_SERVICE?
                        let discovery = MdnsPacket::ServiceDiscovery(MdnsServiceDiscovery {
                            from,
                            query_id: packet.header.id,
                        });
                        Some(discovery)
                    } else {
                        None
                    }
                } else {
                    let resp = MdnsPacket::Response(MdnsResponse::new(packet, from));
                    Some(resp)
                }
            }
            Err(err) => {
                log::warn!("Parsing mdns packet failed: {:?}", err);
                None
            }
        }
    }
}

/// A received mDNS query.
pub struct MdnsQuery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsQuery {
    /// Source address of the packet.
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }

    /// Query id of the packet.
    pub fn query_id(&self) -> u16 {
        self.query_id
    }
}

impl fmt::Debug for MdnsQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsQuery")
            .field("from", self.remote_addr())
            .field("query_id", &self.query_id)
            .finish()
    }
}

/// A received mDNS service discovery query.
pub struct MdnsServiceDiscovery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsServiceDiscovery {
    /// Source address of the packet.
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }

    /// Query id of the packet.
    pub fn query_id(&self) -> u16 {
        self.query_id
    }
}

impl fmt::Debug for MdnsServiceDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsServiceDiscovery")
            .field("from", self.remote_addr())
            .field("query_id", &self.query_id)
            .finish()
    }
}

/// A received mDNS response.
pub struct MdnsResponse {
    peers: Vec<MdnsPeer>,
    from: SocketAddr,
}

impl MdnsResponse {
    /// Creates a new `MdnsResponse` based on the provided `Packet`.
    fn new(packet: Packet<'_>, from: SocketAddr) -> MdnsResponse {
        let peers = packet
            .answers
            .iter()
            .filter_map(|record| {
                if record.name.to_string().as_bytes() != SERVICE_NAME {
                    return None;
                }

                let record_value = match record.data {
                    RData::PTR(record) => record.0.to_string(),
                    _ => return None,
                };

                let mut peer_name = match record_value.rsplitn(4, |c| c == '.').last() {
                    Some(n) => n.to_owned(),
                    None => return None,
                };

                // if we have a segmented name, remove the '.'
                peer_name.retain(|c| c != '.');

                let peer_id = match data_encoding::BASE32_DNSCURVE.decode(peer_name.as_bytes()) {
                    Ok(bytes) => match PeerId::from_bytes(&bytes) {
                        Ok(id) => id,
                        Err(_) => return None,
                    },
                    Err(_) => return None,
                };

                Some(MdnsPeer::new(&packet, record_value, peer_id, record.ttl))
            })
            .collect();

        MdnsResponse { peers, from }
    }

    /// Returns the list of peers that have been reported in this packet.
    ///
    /// > **Note**: Keep in mind that this will also contain the responses we sent ourselves.
    pub fn discovered_peers(&self) -> impl Iterator<Item = &MdnsPeer> {
        self.peers.iter()
    }

    /// Source address of the packet.
    #[inline]
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }
}

impl fmt::Debug for MdnsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsResponse").field("from", self.remote_addr()).finish()
    }
}

/// A peer discovered by the service.
pub struct MdnsPeer {
    addrs: Vec<Multiaddr>,
    /// Id of the peer.
    peer_id: PeerId,
    /// TTL of the record in seconds.
    ttl: u32,
}

impl MdnsPeer {
    /// Creates a new `MdnsPeer` based on the provided `Packet`.
    pub fn new(packet: &Packet<'_>, record_value: String, my_peer_id: PeerId, ttl: u32) -> MdnsPeer {
        let addrs = packet
            .additional
            .iter()
            .filter_map(|add_record| {
                if add_record.name.to_string() != record_value {
                    return None;
                }

                if let RData::TXT(ref txt) = add_record.data {
                    Some(txt)
                } else {
                    None
                }
            })
            .flat_map(|txt| txt.iter())
            .filter_map(|txt| {
                // TODO: wrong, txt can be multiple character strings
                let addr = match dns::decode_character_string(txt) {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                if !addr.starts_with(b"dnsaddr=") {
                    return None;
                }
                let addr = match str::from_utf8(&addr[8..]) {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                let mut addr = match addr.parse::<Multiaddr>() {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                match addr.pop() {
                    Some(Protocol::P2p(peer_id)) => {
                        if let Ok(peer_id) = PeerId::try_from(peer_id) {
                            if peer_id != my_peer_id {
                                return None;
                            }
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                };
                Some(addr)
            })
            .collect();

        MdnsPeer {
            addrs,
            peer_id: my_peer_id,
            ttl,
        }
    }

    /// Returns the id of the peer.
    #[inline]
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the requested time-to-live for the record.
    #[inline]
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(u64::from(self.ttl))
    }

    /// Returns the list of addresses the peer says it is listening on.
    ///
    /// Filters out invalid addresses.
    pub fn addresses(&self) -> &Vec<Multiaddr> {
        &self.addrs
    }
}

impl fmt::Debug for MdnsPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsPeer").field("peer_id", &self.peer_id).finish()
    }
}
