use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::num::NonZeroU8;
use std::net::TcpListener;
use udp_over_tcp::{Udp2Tcp, TcpOptions};

fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 1,
        1 => 1,
        n => fibonacci(n-1) + fibonacci(n-2),
    }
}

fn transfer_1gb() {
    let tcp_listener = TcpListener::bind("127.0.0.1:0").unwrap();

    let rt = create_runtime(Some(NonZeroU8::new(1).unwrap()));
    let udp_listen_addr = "127.0.0.1:5000".parse().unwrap();
    let tcp_forward_addr = "127.0.0.1:5001".parse().unwrap();
    let udp2tcp = Udp2Tcp::new(
        udp_listen_addr,
        tcp_forward_addr,
        TcpOptions::default(),
    );

}

fn criterion_benchmark(c: &mut Criterion) {
    
    c.bench_function("transfer 1GiB", |b| b.iter(|| {
        
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

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
