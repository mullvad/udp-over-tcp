//! Primitives for listening on TCP and forwarding the data in incoming connections
//! to UDP.

use crate::logging::Redact;
use err_context::{BoxedErrorExt as _, ResultExt as _};
use std::convert::Infallible;
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpListener, TcpSocket, TcpStream, UdpSocket};

#[derive(Debug)]
#[cfg_attr(feature = "structopt", derive(structopt::StructOpt))]
pub struct Options {
    /// The IP and TCP port(s) to listen to for incoming traffic from udp2tcp.
    /// Supports binding multiple TCP sockets.
    #[cfg_attr(feature = "structopt", structopt(long = "tcp-listen", required(true)))]
    pub tcp_listen_addrs: Vec<SocketAddr>,

    #[cfg_attr(feature = "structopt", structopt(long = "udp-forward"))]
    /// The IP and UDP port to forward all traffic to.
    pub udp_forward_addr: SocketAddr,

    /// Which local IP to bind the UDP socket to.
    #[cfg_attr(
        feature = "structopt",
        structopt(long = "udp-bind", default_value = "0.0.0.0")
    )]
    pub udp_bind_ip: IpAddr,

    #[cfg_attr(feature = "structopt", structopt(flatten))]
    pub tcp_options: crate::tcp_options::TcpOptions,
}

/// Error returned from [`run`] if something goes wrong.
#[derive(Debug)]
pub enum Tcp2UdpError {
    /// No TCP listen addresses given in the `Options`.
    NoTcpListenAddrs,
    CreateTcpSocket(io::Error),
    /// Failed to apply TCP options to socket.
    ApplyTcpOptions(crate::tcp_options::ApplyTcpOptionsError),
    /// Failed to enable `SO_REUSEADDR` on TCP socket
    SetReuseAddr(io::Error),
    /// Failed to bind TCP socket to SocketAddr
    BindTcpSocket(io::Error, SocketAddr),
    /// Failed to start listening on TCP socket
    ListenTcpSocket(io::Error, SocketAddr),
}

impl fmt::Display for Tcp2UdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Tcp2UdpError::*;
        match self {
            NoTcpListenAddrs => "Invalid options, no TCP listen addresses".fmt(f),
            CreateTcpSocket(_) => "Failed to create TCP socket".fmt(f),
            ApplyTcpOptions(_) => "Failed to apply options to TCP socket".fmt(f),
            SetReuseAddr(_) => "Failed to set SO_REUSEADDR on TCP socket".fmt(f),
            BindTcpSocket(_, addr) => write!(f, "Failed to bind TCP socket to {}", addr),
            ListenTcpSocket(_, addr) => write!(
                f,
                "Failed to start listening on TCP socket bound to {}",
                addr
            ),
        }
    }
}

impl std::error::Error for Tcp2UdpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use Tcp2UdpError::*;
        match self {
            NoTcpListenAddrs => None,
            CreateTcpSocket(e) => Some(e),
            ApplyTcpOptions(e) => Some(e),
            SetReuseAddr(e) => Some(e),
            BindTcpSocket(e, _) => Some(e),
            ListenTcpSocket(e, _) => Some(e),
        }
    }
}

/// Sets up TCP listening sockets on all addresses in `Options::tcp_listen_addrs`.
/// If binding a listening socket fails this returns an error. Otherwise the function
/// will continue indefinitely to accept incoming connections and forward to UDP.
/// Errors are just logged.
pub async fn run(options: Options) -> Result<Infallible, Tcp2UdpError> {
    if options.tcp_listen_addrs.is_empty() {
        return Err(Tcp2UdpError::NoTcpListenAddrs);
    }

    let mut join_handles = Vec::with_capacity(options.tcp_listen_addrs.len());
    for tcp_listen_addr in options.tcp_listen_addrs {
        let tcp_listener = create_listening_socket(tcp_listen_addr, &options.tcp_options)?;
        log::info!("Listening on {}/TCP", tcp_listener.local_addr().unwrap());

        let udp_bind_ip = options.udp_bind_ip;
        let udp_forward_addr = options.udp_forward_addr;
        let tcp_recv_timeout = options.tcp_options.recv_timeout;
        join_handles.push(tokio::spawn(async move {
            process_tcp_listener(
                tcp_listener,
                udp_bind_ip,
                udp_forward_addr,
                tcp_recv_timeout,
            )
            .await;
        }));
    }
    futures::future::join_all(join_handles).await;
    unreachable!("Listening TCP sockets never exit");
}

fn create_listening_socket(
    addr: SocketAddr,
    options: &crate::tcp_options::TcpOptions,
) -> Result<TcpListener, Tcp2UdpError> {
    let tcp_socket = match addr {
        SocketAddr::V4(..) => TcpSocket::new_v4(),
        SocketAddr::V6(..) => TcpSocket::new_v6(),
    }
    .map_err(Tcp2UdpError::CreateTcpSocket)?;
    crate::tcp_options::apply(&tcp_socket, options).map_err(Tcp2UdpError::ApplyTcpOptions)?;
    tcp_socket
        .set_reuseaddr(true)
        .map_err(Tcp2UdpError::SetReuseAddr)?;
    tcp_socket
        .bind(addr)
        .map_err(|e| Tcp2UdpError::BindTcpSocket(e, addr))?;
    let tcp_listener = tcp_socket
        .listen(1024)
        .map_err(|e| Tcp2UdpError::ListenTcpSocket(e, addr))?;

    Ok(tcp_listener)
}

async fn process_tcp_listener(
    tcp_listener: TcpListener,
    udp_bind_ip: IpAddr,
    udp_forward_addr: SocketAddr,
    tcp_recv_timeout: Option<Duration>,
) -> ! {
    loop {
        match tcp_listener.accept().await {
            Ok((tcp_stream, tcp_peer_addr)) => {
                log::debug!("Incoming connection from {}/TCP", Redact(tcp_peer_addr));

                tokio::spawn(async move {
                    if let Err(error) = process_socket(
                        tcp_stream,
                        tcp_peer_addr,
                        udp_bind_ip,
                        udp_forward_addr,
                        tcp_recv_timeout,
                    )
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

/// Sets up a UDP socket bound to `udp_bind_ip` and connected to `udp_peer_addr` and forwards
/// traffic between that UDP socket and the given `tcp_stream` until the `tcp_stream` is closed.
/// `tcp_peer_addr` should be the remote addr that `tcp_stream` is connected to.
async fn process_socket(
    tcp_stream: TcpStream,
    tcp_peer_addr: SocketAddr,
    udp_bind_ip: IpAddr,
    udp_peer_addr: SocketAddr,
    tcp_recv_timeout: Option<Duration>,
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
            .ok()
            .as_ref()
            .map(|item| -> &dyn fmt::Display { &*item })
            .unwrap_or(&"unknown"),
        udp_peer_addr
    );

    crate::forward_traffic::process_udp_over_tcp(udp_socket, tcp_stream, tcp_recv_timeout).await;
    log::debug!(
        "Closing forwarding for {}/TCP <-> {}/UDP",
        Redact(tcp_peer_addr),
        udp_peer_addr
    );

    Ok(())
}
