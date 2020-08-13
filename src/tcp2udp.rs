use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use structopt::StructOpt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

mod shared;
use shared::{process_tcp2udp, process_udp2tcp};

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
struct Options {
    tcp_listen_addr: SocketAddrV4,

    udp_forward_addr: SocketAddrV4,

    #[structopt(long = "udp-bind", default_value = "0.0.0.0")]
    udp_bind_ip: Ipv4Addr,
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
    let mut tcp_listener = TcpListener::bind(options.tcp_listen_addr)
        .await
        .with_context(|_| {
            format!(
                "Failed to bind a TCP listener to {}",
                options.tcp_listen_addr
            )
        })?;
    log::info!("Listening on {}/TCP", tcp_listener.local_addr().unwrap());

    loop {
        match tcp_listener.accept().await {
            Ok((socket, tcp_peer_addr)) => {
                log::debug!("Incoming connection from {}/TCP", tcp_peer_addr);
                let udp_bind_ip = options.udp_bind_ip;
                let udp_forward_addr = options.udp_forward_addr;
                tokio::spawn(async move {
                    if let Err(error) =
                        process_socket(socket, tcp_peer_addr, udp_bind_ip, udp_forward_addr).await
                    {
                        log::error!("Error: {}", error.display("\nCaused by: "));
                    }
                });
            }
            Err(error) => log::error!("Error when accepting incoming TCP connection: {}", error),
        }
    }
}

async fn process_socket(
    socket: TcpStream,
    tcp_peer_addr: SocketAddr,
    udp_bind_ip: Ipv4Addr,
    udp_peer_addr: SocketAddrV4,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tcp_in, tcp_out) = socket.into_split();

    let udp_bind_addr = SocketAddrV4::new(udp_bind_ip, 0);

    let udp_socket = UdpSocket::bind(udp_bind_addr)
        .await
        .with_context(|_| format!("Failed to bind UDP socket to {}", udp_bind_addr))?;
    udp_socket
        .connect(udp_peer_addr)
        .await
        .with_context(|_| format!("Failed to connect UDP socket to {}", udp_peer_addr))?;
    log::debug!(
        "UDP socket bound to {} and connected to {}",
        udp_socket.local_addr()?,
        udp_peer_addr
    );
    let (udp_in, udp_out) = udp_socket.split();

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
                tcp_peer_addr,
                udp_peer_addr
            );
        },
        _ = udp2tcp_future => {
            log::trace!(
                "Closing UDP->TCP end for {}->{}",
                udp_peer_addr,
                tcp_peer_addr
            );
        }
    }
    log::trace!(
        "Closing forwarding for {}/TCP <-> {}/UDP",
        tcp_peer_addr,
        udp_peer_addr
    );

    Ok(())
}
