use err_context::BoxedErrorExt as _;
use structopt::StructOpt;

use udp_over_tcp::tcp2udp;

#[derive(Debug, StructOpt)]
#[structopt(name = "tcp2udp", about = "Listen for incoming TCP and forward to UDP")]
pub struct Options {
    /// Sets the number of worker threads to use.
    /// The default value is the number of cores available to the system.
    #[structopt(long = "threads")]
    threads: Option<std::num::NonZeroU8>,

    #[structopt(flatten)]
    tcp2udp_options: tcp2udp::Options,
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
        .block_on(tcp2udp::run(options.tcp2udp_options));
    if let Err(error) = result {
        log::error!("Error: {}", error.display("\nCaused by: "));
        std::process::exit(1);
    }
}
