// Copyright 2021 Netwarps Ltd.
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

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{AsyncRead, AsyncWrite};

use async_std::net;
use async_std::net::ToSocketAddrs;

#[derive(Debug)]
pub struct TcpStream(net::TcpStream);
#[derive(Debug)]
pub struct TcpListener(net::TcpListener);

impl TcpStream {
    /// Creates a new TCP stream connected to the specified address.
    pub async fn connect<A: ToSocketAddrs>(addrs: A) -> io::Result<TcpStream> {
        net::TcpStream::connect(addrs).await.map(TcpStream)
    }

    /// Gets the inner mutable TcpStream.
    pub fn inner_mut(&mut self) -> &mut net::TcpStream {
        &mut self.0
    }

    /// Returns the local address that this stream is connected to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    /// Returns the remote address that this stream is connected to.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }

    /// Gets the value of the `IP_TTL` option for this socket.
    pub fn ttl(&self) -> io::Result<u32> {
        self.0.ttl()
    }

    /// Sets the value for the `IP_TTL` option on this socket.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.0.set_ttl(ttl)
    }

    /// Gets the value of the `TCP_NODELAY` option on this socket.
    pub fn nodelay(&self) -> io::Result<bool> {
        self.0.nodelay()
    }

    /// Sets the value of the `TCP_NODELAY` option on this socket.
    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.0.set_nodelay(nodelay)
    }
}

impl TcpListener {
    /// Creates a new `TcpListener` which will be bound to the specified address.
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> io::Result<TcpListener> {
        net::TcpListener::bind(addrs).await.map(TcpListener)
    }

    /// Accepts a new incoming connection to this listener.
    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        self.0.accept().await.map(|(t, s)| (TcpStream(t), s))
    }

    /// Returns the local address that this listener is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl From<std::net::TcpStream> for TcpStream {
    /// Converts a `std::net::TcpStream` into its asynchronous equivalent.
    fn from(stream: std::net::TcpStream) -> TcpStream {
        TcpStream(net::TcpStream::from(stream))
    }
}

impl From<std::net::TcpListener> for TcpListener {
    /// Converts a `std::net::TcpListener` into its asynchronous equivalent.
    fn from(listener: std::net::TcpListener) -> TcpListener {
        TcpListener(net::TcpListener::from(listener))
    }
}

// AsyncRead/AsyncWrite
impl AsyncRead for TcpStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.0), cx, buf)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }

    // tokio, poll_close vs. poll_shutdown
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.0), cx)
    }
}

#[derive(Debug)]
pub struct UdpSocket(net::UdpSocket);

impl UdpSocket {
    /// Creates a UDP socket from the given address.
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> io::Result<UdpSocket> {
        net::UdpSocket::bind(addrs).await.map(UdpSocket)
    }

    /// Returns the peer address that this listener is connected to.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }

    /// Returns the local address that this listener is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    /// Sends data on the socket to the given address.
    pub async fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addrs: A) -> io::Result<usize> {
        self.0.send_to(buf, addrs).await
    }

    /// Receives data from the socket.
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.recv_from(buf).await
    }

    /// Receives data from socket without removing it from the queue.
    pub async fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.peek_from(buf).await
    }

    /// Connects the UDP socket to a remote address.
    pub async fn connect<A: ToSocketAddrs>(&self, addrs: A) -> io::Result<()> {
        self.0.connect(addrs).await
    }

    /// Sends data on the socket to the remote address to which it is connected.
    pub async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf).await
    }

    /// Receives data from the socket.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.recv(buf).await
    }

    /// Receives data from the socket without removing it from the queue.
    pub async fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.peek(buf).await
    }

    /// Gets the value of the `SO_BROADCAST` option for this socket.
    pub fn broadcast(&self) -> io::Result<bool> {
        self.0.broadcast()
    }

    /// Sets the value of the `SO_BROADCAST` option for this socket.
    pub fn set_broadcast(&self, on: bool) -> io::Result<()> {
        self.0.set_broadcast(on)
    }

    /// Gets the value of the `IP_MULTICAST_LOOP` option for this socket.
    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        self.0.multicast_loop_v4()
    }

    /// Sets the value of the `IP_MULTICAST_LOOP` option for this socket.
    pub fn set_multicast_loop_v4(&self, on: bool) -> io::Result<()> {
        self.0.set_multicast_loop_v4(on)
    }

    /// Gets the value of the `IP_MULTICAST_TTL` option for this socket.
    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        self.0.multicast_ttl_v4()
    }

    /// Sets the value of the `IP_MULTICAST_TTL` option for this socket.
    pub fn set_multicast_ttl_v4(&self, ttl: u32) -> io::Result<()> {
        self.0.set_multicast_ttl_v4(ttl)
    }

    /// Gets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        self.0.multicast_loop_v6()
    }

    /// Sets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
    pub fn set_multicast_loop_v6(&self, on: bool) -> io::Result<()> {
        self.0.set_multicast_loop_v6(on)
    }

    /// Gets the value of the `IP_TTL` option for this socket.
    pub fn ttl(&self) -> io::Result<u32> {
        self.0.ttl()
    }

    /// Sets the value for the `IP_TTL` option on this socket.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.0.set_ttl(ttl)
    }

    /// Executes an operation of the `IP_ADD_MEMBERSHIP` type.
    pub fn join_multicast_v4(&self, multiaddr: Ipv4Addr, interface: Ipv4Addr) -> io::Result<()> {
        self.0.join_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the `IPV6_ADD_MEMBERSHIP` type.
    pub fn join_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.0.join_multicast_v6(multiaddr, interface)
    }

    /// Executes an operation of the `IP_DROP_MEMBERSHIP` type.
    pub fn leave_multicast_v4(&self, multiaddr: Ipv4Addr, interface: Ipv4Addr) -> io::Result<()> {
        self.0.leave_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the `IPV6_DROP_MEMBERSHIP` type.
    pub fn leave_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.0.leave_multicast_v6(multiaddr, interface)
    }
}

impl From<std::net::UdpSocket> for UdpSocket {
    /// Converts a `std::net::UdpSocket` into its asynchronous equivalent.
    fn from(socket: std::net::UdpSocket) -> UdpSocket {
        UdpSocket(net::UdpSocket::from(socket))
    }
}

///////////////////////////////////////////////////////////////////////////////////////////

/// Performs a DNS resolution.
///
/// Note: we might re-export async_std::net::ToSocketAddrs, which can be used to perform
/// DNS resolution. However, tokio::net::ToSocketAddrs is sealed and is intended to be
/// internally in Tokio.
///
/// Therefore, as a simple and strait solution, we provide this method to do DNS resolution
/// as an alternative for covering both async-std and tokio use cases.
pub async fn resolve_host<T>(host: T) -> io::Result<impl Iterator<Item = SocketAddr>>
where
    T: ToSocketAddrs,
{
    host.to_socket_addrs().await
}
