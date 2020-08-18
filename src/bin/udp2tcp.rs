use err_context::BoxedErrorExt as _;
use structopt::StructOpt;

use udp_over_tcp::udp2tcp;

#[derive(Debug, StructOpt)]
#[structopt(name = "udp2tcp", about = "Listen for incoming UDP and forward to TCP")]
pub struct Options {
    #[structopt(flatten)]
    udp2tcp_options: udp2tcp::Options,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let options = Options::from_args();
    if let Err(error) = udp2tcp::run(options.udp2tcp_options).await {
        log::error!("Error: {}", error.display("\nCaused by: "));
        std::process::exit(1);
    }
}
