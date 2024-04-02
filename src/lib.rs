//! A library (and binaries) for tunneling UDP datagrams over a TCP stream.
//!
//! Some programs/protocols only work over UDP. And some networks only allow TCP. This is where
//! `udp-over-tcp` comes in handy. This library comes in two parts:
//!
//! * `udp2tcp` - Forwards incoming UDP datagrams over a TCP stream. The return stream
//!   is translated back to datagrams and sent back out over UDP again.
//!   This part can be easily used as both a library and a binary.
//!   So it can be run standalone, but can also easily be included in other
//!   Rust programs. The UDP socket is connected to the peer address of the first incoming
//!   datagram. So one [`Udp2Tcp`] instance can handle traffic from a single peer only.
//! * `tcp2udp` - Accepts connections over TCP and translates + forwards the incoming stream
//!   as UDP datagrams to the destination specified during setup / on the command line.
//!   Designed mostly to be a standalone executable to run on servers. But can be
//!   consumed as a Rust library as well.
//!   `tcp2udp` continues to accept new incoming TCP connections, and creates a new UDP socket
//!   for each. So a single `tcp2udp` server can be used to service many `udp2tcp` clients.
//!
//! # Protocol
//!
//! The format of the data inside the TCP stream is very simple. Each datagram is preceded
//! with a 16 bit unsigned integer in big endian byte order, specifying the length of the datagram.
//!
//! # tcp2udp server example
//!
//! Make the server listen for TCP connections that it can then forward to a local UDP service.
//! This will listen on `10.0.0.1:5001/TCP` and forward anything that
//! comes in to `127.0.0.1:51820/UDP`:
//! ```bash
//! user@server $ RUST_LOG=debug tcp2udp \
//!     --tcp-listen 10.0.0.0:5001 \
//!     --udp-forward 127.0.0.1:51820
//! ```
//!
//! `RUST_LOG` can be used to set logging level. See documentation for [`env_logger`] for
//! information. The crate must be built with the `env_logger` feature for this to be active.
//!
//! `REDACT_LOGS=1` can be set to redact the IPs of the peers using the service from the logs.
//! Allows having logging turned on but without storing potentially user sensitive data to disk.
//!
//! [`env_logger`]: https://crates.io/crates/env_logger
//!
//! # udp2tcp example
//!
//! This is one way you could integrate `udp2tcp` into your Rust program.
//! This will connect a TCP socket to `1.2.3.4:9000` and bind a UDP socket to a random port
//! on the loopback interface.
//! It will then connect the UDP socket to the socket addr of the first incoming datagram
//! and start forwarding all traffic to (and from) the TCP socket.
//!
//! ```no_run
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # fn spin_up_some_udp_thing<T>(t: T) {}
//!
//! let udp_listen_addr = "127.0.0.1:0".parse().unwrap();
//! let tcp_forward_addr = "1.2.3.4:9000".parse().unwrap();
//!
//! // Create a UDP -> TCP forwarder. This will connect the TCP socket
//! // to `tcp_forward_addr`
//! let udp2tcp = udp_over_tcp::Udp2Tcp::new(
//!     udp_listen_addr,
//!     tcp_forward_addr,
//!     udp_over_tcp::TcpOptions::default(),
//! )
//! .await?;
//!
//! // Read out which address the UDP actually bound to. Useful if you specified port
//! // zero to get a random port from the OS.
//! let local_udp_addr = udp2tcp.local_udp_addr()?;
//!
//! spin_up_some_udp_thing(local_udp_addr);
//!
//! // Run the forwarder until the TCP socket disconnects or an error happens.
//! udp2tcp.run().await?;
//! # Ok(())
//! # }
//! ```
//!

#![forbid(unsafe_code)]
#![deny(clippy::all)]

pub mod tcp2udp;
pub mod udp2tcp;

pub use udp2tcp::Udp2Tcp;

mod exponential_backoff;
mod forward_traffic;
mod logging;
mod tcp_options;

pub use tcp_options::{ApplyTcpOptionsError, ApplyTcpOptionsErrorKind, TcpOptions};

/// Helper trait for `Result<Infallible, E>` types. Allows getting the `E` value
/// in a way that is guaranteed to not panic.
pub trait NeverOkResult<E> {
    fn into_error(self) -> E;
}

impl<E> NeverOkResult<E> for Result<std::convert::Infallible, E> {
    fn into_error(self) -> E {
        self.expect_err("Result<Infallible, _> can't be Ok variant")
    }
}
