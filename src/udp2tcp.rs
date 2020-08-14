use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::convert::TryFrom;
use std::net::SocketAddrV4;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

mod shared;
use shared::{process_tcp2udp, process_udp2tcp};

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
struct Options {
    #[structopt(long = "udp-listen", default_value = "0.0.0.0:51820")]
    udp_listen_addr: SocketAddrV4,

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
    let mut udp_socket = UdpSocket::bind(options.udp_listen_addr)
        .await
        .with_context(|_| format!("Failed to bind UDP socket to {}", options.udp_listen_addr))?;
    log::info!("Listening on {}/UDP", udp_socket.local_addr().unwrap());

    let mut buffer = [0u8; 1024 * 64];
    let (udp_read_len, udp_peer_addr) = udp_socket
        .recv_from(&mut buffer)
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

    let (udp_in, udp_out) = udp_socket.split();

    let tcp_socket = TcpStream::connect(options.tcp_forward_addr)
        .await
        .with_context(|_| format!("Failed to connect to {}/TCP", options.tcp_forward_addr))?;
    if options.tcp_nodelay {
        tcp_socket
            .set_nodelay(true)
            .context("Failed setting TCP_NODELAY")?;
    }
    log::debug!(
        "TCP_NODELAY: {}",
        tcp_socket.nodelay().context("Failed getting TCP_NODELAY")?
    );
    if let Some(recv_buffer_size) = options.tcp_recv_buffer_size {
        tcp_socket
            .set_recv_buffer_size(recv_buffer_size)
            .context("Failed setting SO_RCVBUF")?;
    }
    log::debug!(
        "SO_RCVBUF: {}",
        tcp_socket
            .recv_buffer_size()
            .context("Failed getting SO_RCVBUF")?
    );
    if let Some(send_buffer_size) = options.tcp_send_buffer_size {
        tcp_socket
            .set_send_buffer_size(send_buffer_size)
            .context("Failed setting SO_SNDBUF")?;
    }
    log::debug!(
        "SO_SNDBUF: {}",
        tcp_socket
            .send_buffer_size()
            .context("Failed getting SO_SNDBUF")?
    );

    let (tcp_in, mut tcp_out) = tcp_socket.into_split();

    let datagram_len = u16::try_from(udp_read_len).unwrap();
    log::trace!("Read {} byte UDP datagram", datagram_len);
    tcp_out.write_u16(datagram_len).await?;
    tcp_out
        .write_all(&buffer[..udp_read_len])
        .await
        .context("Failed writing to TCP")?;
    log::trace!("Forwarded {} bytes UDP->TCP", udp_read_len);

    let tcp2udp_future = async move {
        if let Err(error) = process_tcp2udp(tcp_in, udp_out).await {
            log::error!("Error: {}", error.display("\nCaused by: "));
        }
    };
    let udp2tcp_future = async move {
        if let Err(error) = process_udp2tcp(udp_in, tcp_out).await {
            log::error!("Error: {}", error.display("\nCaused by: "));
        }
    };

    tokio::select! {
        _ = tcp2udp_future => {
            log::trace!(
                "Closing TCP->UDP end for {}->{}",
                options.tcp_forward_addr,
                udp_peer_addr
            );
        },
        _ = udp2tcp_future => {
            log::trace!(
                "Closing UDP->TCP end for {}->{}",
                udp_peer_addr,
                options.tcp_forward_addr
            );
        }
    }
    log::trace!(
        "Closing forwarding for {}/UDP <-> {}/TCP",
        udp_peer_addr,
        options.tcp_forward_addr,
    );

    Ok(())
}
