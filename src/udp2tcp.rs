use err_context::ResultExt as _;
use std::convert::TryFrom;
use std::fmt;
use std::io;
use std::net::SocketAddrV4;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

#[derive(Debug, StructOpt)]
pub struct Options {
    /// The IP and UDP port to bind to and accept incoming connections on.
    pub udp_listen_addr: SocketAddrV4,

    /// The IP and TCP port to forward all UDP traffic to.
    pub tcp_forward_addr: SocketAddrV4,

    #[structopt(flatten)]
    pub tcp_options: crate::tcp_options::TcpOptions,
}

#[derive(Debug)]
pub enum Error {
    ConnectTcp(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            ConnectTcp(_) => "Failed to connect to TCP forward address".fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use Error::*;
        match self {
            ConnectTcp(e) => Some(e),
        }
    }
}

pub async fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut tcp_stream = TcpStream::connect(options.tcp_forward_addr)
        .await
        .map_err(Error::ConnectTcp)?;
    log::info!("Connected to {}/TCP", options.tcp_forward_addr);
    crate::tcp_options::apply(&tcp_stream, &options.tcp_options)?;

    let mut udp_socket = UdpSocket::bind(options.udp_listen_addr)
        .await
        .with_context(|_| format!("Failed to bind UDP socket to {}", options.udp_listen_addr))?;
    log::info!("Listening on {}/UDP", udp_socket.local_addr().unwrap());

    let mut buffer = [0u8; 2 + 1024 * 64];
    let (udp_read_len, udp_peer_addr) = udp_socket
        .recv_from(&mut buffer[2..])
        .await
        .context("Failed receiving the first packet")?;
    log::info!(
        "Incoming connection from {}/UDP, forwarding to {}/TCP",
        udp_peer_addr,
        options.tcp_forward_addr
    );

    udp_socket
        .connect(udp_peer_addr)
        .await
        .with_context(|_| format!("Failed to connect UDP socket to {}", udp_peer_addr))?;

    let datagram_len = u16::try_from(udp_read_len).unwrap();
    buffer[..2].copy_from_slice(&datagram_len.to_be_bytes()[..]);
    tcp_stream
        .write_all(&buffer[..2 + udp_read_len])
        .await
        .context("Failed writing to TCP")?;
    log::trace!("Forwarded {} bytes UDP->TCP", udp_read_len);

    crate::forward_traffic::process_udp_over_tcp(udp_socket, tcp_stream).await;
    log::trace!(
        "Closing forwarding for {}/UDP <-> {}/TCP",
        udp_peer_addr,
        options.tcp_forward_addr,
    );

    Ok(())
}
