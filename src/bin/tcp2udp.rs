use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use structopt::StructOpt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
struct Options {
    /// The IP and TCP port to listen to for incoming traffic from udp2tcp.
    tcp_listen_addr: SocketAddrV4,

    /// The IP and UDP port to forward all traffic to.
    udp_forward_addr: SocketAddrV4,

    #[structopt(long = "udp-bind", default_value = "0.0.0.0")]
    udp_bind_ip: Ipv4Addr,

    /// Sets the number of worker threads to use.
    /// The default value is the number of cores available to the system.
    #[structopt(long = "threads")]
    threads: Option<std::num::NonZeroU8>,

    #[structopt(flatten)]
    tcp_options: udp_over_tcp::TcpOptions,
}

fn main() {
    env_logger::init();
    let options = Options::from_args();

    let mut rt_builder = tokio::runtime::Builder::new();
    rt_builder.threaded_scheduler().enable_all();
    if let Some(threads) = options.threads {
        log::info!("Using {} threads", threads);
        let threads = usize::from(threads.get());
        rt_builder.core_threads(threads).max_threads(threads);
    }
    let result = rt_builder
        .build()
        .expect("Failed to build async runtime")
        .block_on(run(options));
    if let Err(error) = result {
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
            Ok((tcp_stream, tcp_peer_addr)) => {
                log::debug!("Incoming connection from {}/TCP", tcp_peer_addr);
                udp_over_tcp::apply_tcp_options(&tcp_stream, &options.tcp_options)?;

                let udp_bind_ip = options.udp_bind_ip;
                let udp_forward_addr = options.udp_forward_addr;
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
    udp_bind_ip: Ipv4Addr,
    udp_peer_addr: SocketAddrV4,
) -> Result<(), Box<dyn std::error::Error>> {
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
        udp_socket
            .local_addr()
            .context("Failed getting local address for UDP socket")?,
        udp_peer_addr
    );

    udp_over_tcp::process_udp_over_tcp(udp_socket, tcp_stream).await;
    log::trace!(
        "Closing forwarding for {}/TCP <-> {}/UDP",
        tcp_peer_addr,
        udp_peer_addr
    );
    Ok(())
}
