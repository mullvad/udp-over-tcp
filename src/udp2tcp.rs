//! Primitives for listening on UDP and forwarding the data in incoming datagrams
//! to a TCP stream.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use tokio::net::{TcpSocket, TcpStream, UdpSocket};

#[derive(Debug)]
pub enum ConnectError {
    /// Failed to create the TCP socket.
    CreateTcpSocket(io::Error),
    /// Failed to connect to TCP forward address.
    ConnectTcp(io::Error),
    /// Failed to apply the given TCP socket options.
    ApplyTcpOptions(crate::tcp_options::ApplyTcpOptionsError),
    /// Failed to bind UDP socket locally.
    BindUdp(io::Error),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ConnectError::*;
        match self {
            CreateTcpSocket(_) => "Failed to create the TCP socket".fmt(f),
            ConnectTcp(_) => "Failed to connect to TCP forward address".fmt(f),
            ApplyTcpOptions(e) => e.fmt(f),
            BindUdp(_) => "Failed to bind UDP socket locally".fmt(f),
        }
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use ConnectError::*;
        match self {
            CreateTcpSocket(e) => Some(e),
            ConnectTcp(e) => Some(e),
            ApplyTcpOptions(e) => e.source(),
            BindUdp(e) => Some(e),
        }
    }
}

#[derive(Debug)]
pub enum ForwardError {
    ReadUdp(io::Error),
    ConnectUdp(io::Error),
}

impl fmt::Display for ForwardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ForwardError::*;
        match self {
            ReadUdp(_) => "Failed receiving the first UDP datagram".fmt(f),
            ConnectUdp(_) => "Failed to connect UDP socket to peer".fmt(f),
        }
    }
}

impl std::error::Error for ForwardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use ForwardError::*;
        match self {
            ReadUdp(e) => Some(e),
            ConnectUdp(e) => Some(e),
        }
    }
}

/// Struct allowing listening on UDP and forwarding the traffic over TCP.
pub struct Udp2Tcp {
    tcp_stream: TcpStream,
    udp_socket: UdpSocket,
    tcp_forward_addr: SocketAddr,
}

impl Udp2Tcp {
    /// Connects to the given TCP address and binds to the given UDP address.
    /// Just calling this constructor won't forward any traffic over the sockets (see `run`).
    pub async fn new(
        udp_listen_addr: SocketAddr,
        tcp_forward_addr: SocketAddr,
        tcp_options: crate::TcpOptions,
    ) -> Result<Self, ConnectError> {
        let tcp_stream = Self::connect_tcp_socket(tcp_forward_addr, tcp_options).await?;
        log::info!("Connected to {}/TCP", tcp_forward_addr);

        let udp_socket = UdpSocket::bind(udp_listen_addr)
            .await
            .map_err(ConnectError::BindUdp)?;
        match udp_socket.local_addr() {
            Ok(addr) => log::info!("Listening on {}/UDP", addr),
            Err(e) => log::error!("Unable to get UDP local addr: {}", e),
        }

        Ok(Self {
            tcp_stream,
            udp_socket,
            tcp_forward_addr,
        })
    }

    async fn connect_tcp_socket(
        addr: SocketAddr,
        options: crate::TcpOptions,
    ) -> Result<TcpStream, ConnectError> {
        let tcp_socket = match addr {
            SocketAddr::V4(..) => TcpSocket::new_v4(),
            SocketAddr::V6(..) => TcpSocket::new_v6(),
        }
        .map_err(ConnectError::CreateTcpSocket)?;

        crate::tcp_options::apply(&tcp_socket, &options).map_err(ConnectError::ApplyTcpOptions)?;

        let tcp_stream = tcp_socket
            .connect(addr)
            .await
            .map_err(ConnectError::ConnectTcp)?;
        Ok(tcp_stream)
    }

    /// Returns the UDP address this instance is listening on for incoming datagrams to forward.
    ///
    /// Useful to call if `Udp2Tcp::new` was given port zero in `udp_listen_addr` to let the OS
    /// pick a random port. Then this method will return the actual port it is now bound to.
    pub fn local_udp_addr(&self) -> io::Result<SocketAddr> {
        self.udp_socket.local_addr()
    }

    /// Runs the forwarding until the TCP socket is closed, or an error occur.
    pub async fn run(self) -> Result<(), ForwardError> {
        // Wait for the first datagram, to get the UDP peer_addr to connect to.
        let (_udp_read_len, udp_peer_addr) = self
            .udp_socket
            .peek_from(&mut [0u8; crate::forward_traffic::MAX_DATAGRAM_SIZE])
            .await
            .map_err(ForwardError::ReadUdp)?;
        log::info!("Incoming connection from {}/UDP", udp_peer_addr);

        // Connect the UDP socket to whoever sent the first datagram. This is where
        // all the returned traffic will be sent to.
        self.udp_socket
            .connect(udp_peer_addr)
            .await
            .map_err(ForwardError::ConnectUdp)?;

        crate::forward_traffic::process_udp_over_tcp(self.udp_socket, self.tcp_stream).await;
        log::debug!(
            "Closing forwarding for {}/UDP <-> {}/TCP",
            udp_peer_addr,
            self.tcp_forward_addr,
        );

        Ok(())
    }
}
