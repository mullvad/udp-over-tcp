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

    let (tcp_in, mut tcp_out) = TcpStream::connect(options.tcp_forward_addr)
        .await
        .with_context(|_| format!("Failed to connect to {}/TCP", options.tcp_forward_addr))?
        .into_split();

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
