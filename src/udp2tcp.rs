//! Primitives for listening on UDP and forwarding the data in incoming datagrams
//! to a TCP stream.

use std::convert::TryFrom;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

#[derive(Debug)]
pub enum ConnectError {
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
    WriteTcp(io::Error),
}

impl fmt::Display for ForwardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ForwardError::*;
        match self {
            ReadUdp(_) => "Failed receiving the first UDP datagram".fmt(f),
            ConnectUdp(_) => "Failed to connect UDP socket to peer".fmt(f),
            WriteTcp(_) => "Failed to write first datagram to TCP socket".fmt(f),
        }
    }
}

impl std::error::Error for ForwardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use ForwardError::*;
        match self {
            ReadUdp(e) => Some(e),
            ConnectUdp(e) => Some(e),
            WriteTcp(e) => Some(e),
        }
    }
}

/// Struct allowing listening on UDP and forwarding the traffic over TCP.
pub struct Udp2Tcp {
    tcp_stream: TcpStream,
    udp_socket: UdpSocket,
}

impl Udp2Tcp {
    /// Connects to the given TCP address and binds to the given UDP address.
    /// Just calling this constructor won't forward any traffic over the sockets (see `run`).
    pub async fn new(
        udp_listen_addr: SocketAddr,
        tcp_forward_addr: SocketAddr,
        //tcp_options: Option<&crate::TcpOptions>,
    ) -> Result<Self, ConnectError> {
        let tcp_stream = TcpStream::connect(tcp_forward_addr)
            .await
            .map_err(ConnectError::ConnectTcp)?;
        log::info!("Connected to {}/TCP", tcp_forward_addr);
        // if let Some(tcp_options) = tcp_options {
        //     crate::tcp_options::apply(&tcp_stream, tcp_options)
        //         .map_err(ConnectError::ApplyTcpOptions)?;
        // }

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
        })
    }

    /// Returns the UDP address this instance is listening on for incoming datagrams to forward.
    ///
    /// Useful to call if `Udp2Tcp::new` was given port zero in `udp_listen_addr` to let the OS
    /// pick a random port. Then this method will return the actual port it is now bound to.
    pub fn local_udp_addr(&self) -> io::Result<SocketAddr> {
        self.udp_socket.local_addr()
    }

    /// Runs the forwarding until one of the sockets are closed.
    pub async fn run(mut self) -> Result<(), ForwardError> {
        let mut buffer = [0u8; u16::MAX as usize];

        // Read the first incoming UDP packet.
        let (udp_read_len, udp_peer_addr) = self
            .udp_socket
            // First two bytes reserved for datagram length header
            .recv_from(&mut buffer[2..])
            .await
            .map_err(ForwardError::ReadUdp)?;
        log::info!("Incoming connection from {}/UDP", udp_peer_addr);
        if udp_read_len == 0 {
            log::info!("UDP socket closed");
            return Ok(());
        }

        // Connect the UDP socket to whoever sent the first packet. Allows us to process
        // data from a single client.
        self.udp_socket
            .connect(udp_peer_addr)
            .await
            .map_err(ForwardError::ConnectUdp)?;

        // Set the "header" to the length of the datagram.
        let datagram_len =
            u16::try_from(udp_read_len).expect("UDP datagram can't be larger than 2^16");
        buffer[..2].copy_from_slice(&datagram_len.to_be_bytes()[..]);

        self.tcp_stream
            .write_all(&buffer[..2 + udp_read_len])
            .await
            .map_err(ForwardError::WriteTcp)?;
        log::trace!("Forwarded {} bytes UDP->TCP", udp_read_len);

        let tcp_forward_addr = match self.tcp_stream.peer_addr() {
            Ok(addr) => addr.to_string(),
            Err(_e) => "unknown".to_owned(),
        };

        crate::forward_traffic::process_udp_over_tcp(self.udp_socket, self.tcp_stream).await;
        log::trace!(
            "Closing forwarding for {}/UDP <-> {}/TCP",
            udp_peer_addr,
            tcp_forward_addr,
        );

        Ok(())
    }
}
