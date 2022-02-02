#![forbid(unsafe_code)]

use err_context::ErrorExt as _;
use std::num::NonZeroU8;
use structopt::StructOpt;

use udp_over_tcp::{tcp2udp, NeverOkResult};

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
    #[cfg(feature = "env_logger")]
    env_logger::init();

    let options = Options::from_args();

    let runtime = create_runtime(options.threads);

    let error = runtime
        .block_on(tcp2udp::run(options.tcp2udp_options))
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
    runtime.enable_io();

    runtime.build().expect("Failed to build async runtime")
}
