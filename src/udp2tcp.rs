//! Primitives for listening on UDP and forwarding the data in incoming datagrams
//! to a TCP stream.

use crate::logging::Redact;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use tokio::net::{TcpSocket, UdpSocket};

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};

#[derive(Debug)]
pub enum Error {
    /// Failed to create the TCP socket.
    CreateTcpSocket(io::Error),
    /// Failed to apply the given TCP socket options.
    ApplyTcpOptions(crate::tcp_options::ApplyTcpOptionsError),
    /// Failed to bind UDP socket locally.
    BindUdp(io::Error),
    /// Failed to read from UDP socket.
    ReadUdp(io::Error),
    /// Failed to connect UDP socket to the incoming address.
    ConnectUdp(io::Error),
    /// Failed to connect TCP socket to forward address.
    ConnectTcp(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            CreateTcpSocket(_) => "Failed to create the TCP socket".fmt(f),
            ApplyTcpOptions(e) => e.fmt(f),
            BindUdp(_) => "Failed to bind UDP socket locally".fmt(f),
            ReadUdp(_) => "Failed receiving the first UDP datagram".fmt(f),
            ConnectUdp(_) => "Failed to connect UDP socket to peer".fmt(f),
            ConnectTcp(_) => "Failed to connect to TCP forward address".fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use Error::*;
        match self {
            CreateTcpSocket(e) => Some(e),
            ApplyTcpOptions(e) => e.source(),
            BindUdp(e) => Some(e),
            ReadUdp(e) => Some(e),
            ConnectUdp(e) => Some(e),
            ConnectTcp(e) => Some(e),
        }
    }
}

/// Struct allowing listening on UDP and forwarding the traffic over TCP.
pub struct Udp2Tcp {
    tcp_socket: TcpSocket,
    udp_socket: UdpSocket,
    tcp_forward_addr: SocketAddr,
    tcp_options: crate::TcpOptions,
}

impl Udp2Tcp {
    /// Creates a TCP socket and binds to the given UDP address.
    /// Just calling this constructor won't forward any traffic over the sockets (see `run`).
    pub async fn new(
        udp_listen_addr: SocketAddr,
        tcp_forward_addr: SocketAddr,
        tcp_options: crate::TcpOptions,
    ) -> Result<Self, Error> {
        let tcp_socket = match &tcp_forward_addr {
            SocketAddr::V4(..) => TcpSocket::new_v4(),
            SocketAddr::V6(..) => TcpSocket::new_v6(),
        }
        .map_err(Error::CreateTcpSocket)?;

        crate::tcp_options::apply(&tcp_socket, &tcp_options).map_err(Error::ApplyTcpOptions)?;

        let udp_socket = UdpSocket::bind(udp_listen_addr)
            .await
            .map_err(Error::BindUdp)?;
        match udp_socket.local_addr() {
            Ok(addr) => log::info!("Listening on {}/UDP", addr),
            Err(e) => log::error!("Unable to get UDP local addr: {}", e),
        }

        Ok(Self {
            tcp_socket,
            udp_socket,
            tcp_forward_addr,
            tcp_options,
        })
    }

    /// Returns the UDP address this instance is listening on for incoming datagrams to forward.
    ///
    /// Useful to call if `Udp2Tcp::new` was given port zero in `udp_listen_addr` to let the OS
    /// pick a random port. Then this method will return the actual port it is now bound to.
    pub fn local_udp_addr(&self) -> io::Result<SocketAddr> {
        self.udp_socket.local_addr()
    }

    /// Returns the raw file descriptor for the TCP socket that datagrams are forwarded to.
    #[cfg(unix)]
    pub fn remote_tcp_fd(&self) -> RawFd {
        self.tcp_socket.as_raw_fd()
    }

    /// Connects to the TCP address and runs the forwarding until the TCP socket is closed, or
    /// an error occur.
    pub async fn run(self) -> Result<(), Error> {
        // Wait for the first datagram, to get the UDP peer_addr to connect to.
        let mut tmp_buffer = crate::forward_traffic::datagram_buffer();
        let (_udp_read_len, udp_peer_addr) = self
            .udp_socket
            .peek_from(tmp_buffer.as_mut())
            .await
            .map_err(Error::ReadUdp)?;
        log::info!("Incoming connection from {}/UDP", Redact(udp_peer_addr));

        log::info!("Connecting to {}/TCP", self.tcp_forward_addr);
        let tcp_stream = self
            .tcp_socket
            .connect(self.tcp_forward_addr)
            .await
            .map_err(Error::ConnectTcp)?;
        log::info!("Connected to {}/TCP", self.tcp_forward_addr);

        crate::tcp_options::set_nodelay(&tcp_stream, self.tcp_options.nodelay)
            .map_err(Error::ApplyTcpOptions)?;

        // Connect the UDP socket to whoever sent the first datagram. This is where
        // all the returned traffic will be sent to.
        self.udp_socket
            .connect(udp_peer_addr)
            .await
            .map_err(Error::ConnectUdp)?;

        crate::forward_traffic::process_udp_over_tcp(
            self.udp_socket,
            tcp_stream,
            self.tcp_options.recv_timeout,
        )
        .await;
        log::debug!(
            "Closing forwarding for {}/UDP <-> {}/TCP",
            Redact(udp_peer_addr),
            self.tcp_forward_addr,
        );

        Ok(())
    }
}
