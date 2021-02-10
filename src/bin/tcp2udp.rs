use err_context::BoxedErrorExt as _;
use std::num::NonZeroU8;
use structopt::StructOpt;

use udp_over_tcp::tcp2udp;

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
pub struct Options {
    /// Sets the number of worker threads to use.
    /// The default value is the number of cores available to the system.
    #[structopt(long = "threads")]
    threads: Option<NonZeroU8>,

    #[structopt(flatten)]
    tcp2udp_options: tcp2udp::Options,
}

fn main() {
    env_logger::init();
    let options = Options::from_args();

    let mut runtime = match options.threads.map(NonZeroU8::get) {
        Some(1) => {
            log::info!("Using a single thread");
            tokio::runtime::Builder::new_current_thread()
        }
        Some(threads) => {
            let mut runtime = tokio::runtime::Builder::new_multi_thread();
            log::info!("Using {} threads", threads);
            runtime.worker_threads(usize::from(threads));
            runtime
        }
        None => tokio::runtime::Builder::new_multi_thread(),
    };
    runtime.enable_io();

    let result = runtime
        .build()
        .expect("Failed to build async runtime")
        .block_on(tcp2udp::run(options.tcp2udp_options));
    if let Err(error) = result {
        log::error!("Error: {}", error.display("\nCaused by: "));
        std::process::exit(1);
    }
}
