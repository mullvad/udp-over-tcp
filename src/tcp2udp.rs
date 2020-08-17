use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use structopt::StructOpt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

mod shared;

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
struct Options {
    /// The IP and TCP port to listen to for incoming traffic from udp2tcp.
    tcp_listen_addr: SocketAddrV4,

    /// The IP and UDP port to forward all traffic to.
    udp_forward_addr: SocketAddrV4,

    #[structopt(long = "udp-bind", default_value = "0.0.0.0")]
    udp_bind_ip: Ipv4Addr,

    /// Sets the TCP_NODELAY option on the TCP socket.
    /// If set, this option disables the Nagle algorithm.
    /// This means that segments are always sent as soon as possible.
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

    /// Sets the number of worker threads to use.
    /// The default value is the number of cores available to the system.
    #[structopt(long = "threads")]
    threads: Option<std::num::NonZeroU8>,
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
        udp_socket.local_addr()?,
        udp_peer_addr
    );

    shared::process_udp_over_tcp(udp_socket, tcp_stream).await;
    log::trace!(
        "Closing forwarding for {}/TCP <-> {}/UDP",
        tcp_peer_addr,
        udp_peer_addr
    );
    Ok(())
}
