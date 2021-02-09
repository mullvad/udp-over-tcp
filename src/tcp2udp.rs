//! Primitives for listening on TCP and forwarding the data in incoming connections
//! to UDP.

use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::net::{IpAddr, SocketAddr};
use structopt::StructOpt;
use tokio::net::{TcpListener, TcpSocket, TcpStream, UdpSocket};

#[derive(Debug, StructOpt)]
pub struct Options {
    /// The IP and TCP port to listen to for incoming traffic from udp2tcp.
    #[structopt(long = "tcp-listen")]
    pub tcp_listen_addrs: Vec<SocketAddr>,

    #[structopt(long = "udp-forward")]
    /// The IP and UDP port to forward all traffic to.
    pub udp_forward_addr: SocketAddr,

    /// Which local IP to bind the UDP socket to.
    #[structopt(long = "udp-bind", default_value = "0.0.0.0")]
    pub udp_bind_ip: IpAddr,

    #[structopt(flatten)]
    pub tcp_options: crate::tcp_options::TcpOptions,
}

pub async fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    if options.tcp_listen_addrs.is_empty() {
        todo!("Properly handle no TCP listening addresses given");
    }
    let mut join_handles = Vec::with_capacity(options.tcp_listen_addrs.len());
    for tcp_listen_addr in options.tcp_listen_addrs {
        let tcp_listener = create_listening_socket(tcp_listen_addr, &options.tcp_options)?;
        log::info!("Listening on {}/TCP", tcp_listener.local_addr().unwrap());

        let udp_bind_ip = options.udp_bind_ip;
        let udp_forward_addr = options.udp_forward_addr;
        join_handles.push(tokio::spawn(async move {
            process_tcp_listener(tcp_listener, udp_bind_ip, udp_forward_addr).await;
        }));
    }
    futures::future::join_all(join_handles).await;
    Ok(())
}

fn create_listening_socket(
    addr: SocketAddr,
    options: &crate::tcp_options::TcpOptions,
) -> Result<TcpListener, Box<dyn std::error::Error>> {
    let tcp_socket = match addr {
        SocketAddr::V4(..) => TcpSocket::new_v4(),
        SocketAddr::V6(..) => TcpSocket::new_v6(),
    }
    .context("Failed to create new TCP socket")?;
    crate::tcp_options::apply(&tcp_socket, options)?;
    tcp_socket
        .set_reuseaddr(true)
        .context("Failed to set SO_REUSEADDR on TCP socket")?;
    tcp_socket
        .bind(addr)
        .with_context(|_| format!("Failed to bind TCP socket to {}", addr))?;
    let tcp_listener = tcp_socket.listen(1024)?;

    Ok(tcp_listener)
}

async fn process_tcp_listener(
    tcp_listener: TcpListener,
    udp_bind_ip: IpAddr,
    udp_forward_addr: SocketAddr,
) {
    loop {
        match tcp_listener.accept().await {
            Ok((tcp_stream, tcp_peer_addr)) => {
                log::debug!("Incoming connection from {}/TCP", tcp_peer_addr);

                let udp_bind_ip = udp_bind_ip;
                let udp_forward_addr = udp_forward_addr;
                tokio::spawn(async move {
                    if let Err(error) =
                        process_socket(tcp_stream, tcp_peer_addr, udp_bind_ip, udp_forward_addr)
                            .await
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
    tcp_stream: TcpStream,
    tcp_peer_addr: SocketAddr,
    udp_bind_ip: IpAddr,
    udp_peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let udp_bind_addr = SocketAddr::new(udp_bind_ip, 0);

    let udp_socket = UdpSocket::bind(udp_bind_addr)
        .await
        .with_context(|_| format!("Failed to bind UDP socket to {}", udp_bind_addr))?;
    udp_socket
        .connect(udp_peer_addr)
        .await
        .with_context(|_| format!("Failed to connect UDP socket to {}", udp_peer_addr))?;
    log::debug!(
        "UDP socket bound to {} and connected to {}",
        udp_socket
            .local_addr()
            .context("Failed getting local address for UDP socket")?,
        udp_peer_addr
    );

    crate::forward_traffic::process_udp_over_tcp(udp_socket, tcp_stream).await;
    log::trace!(
        "Closing forwarding for {}/TCP <-> {}/UDP",
        tcp_peer_addr,
        udp_peer_addr
    );
    Ok(())
}
