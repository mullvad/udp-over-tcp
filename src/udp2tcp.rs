use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::convert::TryFrom;
use std::net::SocketAddrV4;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

mod shared;

#[derive(Debug, StructOpt)]
#[structopt(name = "udp2tcp", about = "Listen for incoming UDP and forward to TCP")]
struct Options {
    /// The IP and UDP port to bind to and accept incoming connections on.
    udp_listen_addr: SocketAddrV4,

    /// The IP and TCP port to forward all UDP traffic to.
    tcp_forward_addr: SocketAddrV4,

    /// Sets the TCP_NODELAY option on the TCP socket.
    /// If set, this option disables the Nagle algorithm.
    /// This means that segments are always sent as soon as possible
    #[structopt(long = "nodelay")]
    tcp_nodelay: bool,

    /// If given, sets the SO_RCVBUF option on the TCP socket to the given value.
    /// Changes the size of the operating system's receive buffer associated with the socket.
    #[structopt(long = "recv-buffer")]
    tcp_recv_buffer_size: Option<usize>,

    /// If given, sets the SO_SNDBUF option on the TCP socket to the given value.
    /// Changes the size of the operating system's send buffer associated with the socket.
    #[structopt(long = "send-buffer")]
    tcp_send_buffer_size: Option<usize>,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let options = Options::from_args();
    if let Err(error) = run(options).await {
        log::error!("Error: {}", error.display("\nCaused by: "));
        std::process::exit(1);
    }
}

async fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut tcp_stream = TcpStream::connect(options.tcp_forward_addr)
        .await
        .with_context(|_| format!("Failed to connect to {}/TCP", options.tcp_forward_addr))?;
    log::info!("Connected to {}/TCP", options.tcp_forward_addr);
    if options.tcp_nodelay {
        tcp_stream
            .set_nodelay(true)
            .context("Failed setting TCP_NODELAY")?;
    }
    log::debug!(
        "TCP_NODELAY: {}",
        tcp_stream.nodelay().context("Failed getting TCP_NODELAY")?
    );
    if let Some(recv_buffer_size) = options.tcp_recv_buffer_size {
        tcp_stream
            .set_recv_buffer_size(recv_buffer_size)
            .context("Failed setting SO_RCVBUF")?;
    }
    log::debug!(
        "SO_RCVBUF: {}",
        tcp_stream
            .recv_buffer_size()
            .context("Failed getting SO_RCVBUF")?
    );
    if let Some(send_buffer_size) = options.tcp_send_buffer_size {
        tcp_stream
            .set_send_buffer_size(send_buffer_size)
            .context("Failed setting SO_SNDBUF")?;
    }
    log::debug!(
        "SO_SNDBUF: {}",
        tcp_stream
            .send_buffer_size()
            .context("Failed getting SO_SNDBUF")?
    );

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

    shared::process_udp_over_tcp(udp_socket, tcp_stream).await;
    log::trace!(
        "Closing forwarding for {}/UDP <-> {}/TCP",
        udp_peer_addr,
        options.tcp_forward_addr,
    );

    Ok(())
}
