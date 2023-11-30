#![forbid(unsafe_code)]

use clap::Parser;
use err_context::ErrorExt as _;
use std::num::NonZeroU8;

use udp_over_tcp::{tcp2udp, NeverOkResult};

#[derive(Debug, Parser)]
#[command(
    name = "tcp2udp",
    about = "Listen for incoming TCP and forward to UDP",
    version
)]
pub struct Options {
    /// Sets the number of worker threads to use.
    /// The default value is the number of cores available to the system.
    #[arg(long = "threads")]
    threads: Option<NonZeroU8>,

    #[clap(flatten)]
    tcp2udp_options: tcp2udp::Options,

    #[cfg(feature = "statsd")]
    /// Host to send statsd metrics to.
    #[clap(long)]
    statsd_host: Option<std::net::SocketAddr>,
}

fn main() {
    #[cfg(feature = "env_logger")]
    env_logger::init();

    let options = Options::parse();

    #[cfg(feature = "statsd")]
    let statsd_host = options.statsd_host;
    #[cfg(not(feature = "statsd"))]
    let statsd_host = None;

    let runtime = create_runtime(options.threads);

    let error = runtime
        .block_on(tcp2udp::run(options.tcp2udp_options, statsd_host))
        .into_error();
    log::error!("Error: {}", error.display("\nCaused by: "));
    std::process::exit(1);
}

/// Creates a Tokio runtime for the process to use.
/// If `threads` is `None` it uses the same amount of worker threads as system cores.
/// Creates a single threaded runtime if `threads` is `Some(1)`.
/// Otherwise it uses the specified number of worker threads.
fn create_runtime(threads: Option<NonZeroU8>) -> tokio::runtime::Runtime {
    let mut runtime = match threads.map(NonZeroU8::get) {
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
    runtime.enable_time();
    runtime.enable_io();

    runtime.build().expect("Failed to build async runtime")
}
