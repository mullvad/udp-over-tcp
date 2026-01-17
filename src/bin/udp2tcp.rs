#![forbid(unsafe_code)]

use clap::Parser;
use err_context::BoxedErrorExt as _;
use std::net::SocketAddr;

use udp_over_tcp::udp2tcp;

#[derive(Debug, Parser)]
#[command(
    name = "udp2tcp",
    about = "Listen for incoming UDP and forward to TCP",
    version
)]
pub struct Options {
    /// The IP and UDP port to bind to and accept incoming connections on.
    #[arg(long = "udp-listen")]
    pub udp_listen_addr: SocketAddr,

    /// The IP and TCP port to forward all UDP traffic to.
    #[arg(long = "tcp-forward")]
    pub tcp_forward_addr: SocketAddr,

    /// The socks server to use for forwarding TCP traffic
    #[arg(long = "socks-server", env = "MULLVAD_UDP_OVER_TCP_SOCKS_SERVER")]
    pub socks_server_addr: Option<SocketAddr>,

    #[clap(flatten)]
    pub tcp_options: udp_over_tcp::TcpOptions,
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "env_logger")]
    env_logger::init();

    let options = Options::parse();
    if let Err(error) = run(options).await {
        log::error!("Error: {}", error.display("\nCaused by: "));
        std::process::exit(1);
    }
}

async fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let udp2tcp = udp2tcp::Udp2Tcp::new(
        options.udp_listen_addr,
        options.tcp_forward_addr,
        options.socks_server_addr,
        options.tcp_options,
    )
    .await?;
    udp2tcp.run().await?;
    Ok(())
}
