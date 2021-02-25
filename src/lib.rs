//! A library for tunneling UDP datagrams over a TCP stream.
//!
//! The protocol is very simple. Each datagram sent on the TCP stream is prefixed with a 16 bit
//! unsigned integer containing the length of that datagram.

pub mod tcp2udp;
pub mod udp2tcp;

pub use udp2tcp::Udp2Tcp;

mod forward_traffic;
mod tcp_options;

pub use tcp_options::{ApplyTcpOptionsError, TcpOptions};
