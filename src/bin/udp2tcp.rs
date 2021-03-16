use err_context::BoxedErrorExt as _;
use std::net::SocketAddr;
use structopt::StructOpt;

use udp_over_tcp::udp2tcp;

#[derive(Debug, StructOpt)]
#[structopt(name = "udp2tcp", about = "Listen for incoming UDP and forward to TCP")]
pub struct Options {
    /// The IP and UDP port to bind to and accept incoming connections on.
    #[structopt(long = "udp-listen")]
    pub udp_listen_addr: SocketAddr,

    /// The IP and TCP port to forward all UDP traffic to.
    #[structopt(long = "tcp-forward")]
    pub tcp_forward_addr: SocketAddr,

    #[structopt(flatten)]
    pub tcp_options: udp_over_tcp::TcpOptions,
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
    let udp2tcp = udp2tcp::Udp2Tcp::new(
        options.udp_listen_addr,
        options.tcp_forward_addr,
        Some(options.tcp_options),
    )
    .await?;
    udp2tcp.run().await;
    Ok(())
}
