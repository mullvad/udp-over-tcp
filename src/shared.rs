use err_context::BoxedErrorExt as _;
use err_context::ResultExt as _;
use futures::future::{abortable, select, Either};
use std::convert::TryFrom;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf as TcpReadHalf, OwnedWriteHalf as TcpWriteHalf};
use tokio::net::udp::{RecvHalf as UdpRecvHalf, SendHalf as UdpSendHalf};
use tokio::net::{TcpStream, UdpSocket};

#[path = "tcp_options.rs"]
mod tcp_options;
pub use tcp_options::*;

const MAX_DATAGRAM_SIZE: usize = u16::MAX as usize;

/// Forward traffic between the given UDP and TCP sockets in both directions.
/// This async function runs until one of the sockets are closed or there is an error.
/// Both sockets are closed before returning.
pub async fn process_udp_over_tcp(udp_socket: UdpSocket, tcp_stream: TcpStream) {
    let (udp_in, udp_out) = udp_socket.split();
    let (tcp_in, tcp_out) = tcp_stream.into_split();

    let (tcp2udp_future, tcp2udp_abort) = abortable(async move {
        if let Err(error) = process_tcp2udp(tcp_in, udp_out).await {
            log::error!("Error: {}", error.display("\nCaused by: "));
        }
    });
    let (udp2tcp_future, udp2tcp_abort) = abortable(async move {
        if let Err(error) = process_udp2tcp(udp_in, tcp_out).await {
            log::error!("Error: {}", error.display("\nCaused by: "));
        }
    });
    let tcp2udp_join = tokio::spawn(tcp2udp_future);
    let udp2tcp_join = tokio::spawn(udp2tcp_future);

    // Wait until the UDP->TCP or TCP->UDP future terminates, then abort the other.
    let remaining_join_handle = match select(tcp2udp_join, udp2tcp_join).await {
        Either::Left((_, udp2tcp_join)) => udp2tcp_join,
        Either::Right((_, tcp2udp_join)) => tcp2udp_join,
    };
    tcp2udp_abort.abort();
    udp2tcp_abort.abort();
    // Wait for the remaining future to finish. So everything this function spawned
    // is cleaned up before we return.
    let _ = remaining_join_handle.await;
}

async fn process_tcp2udp(
    tcp_in: TcpReadHalf,
    mut udp_out: UdpSendHalf,
) -> Result<(), Box<dyn std::error::Error>> {
    let tcp_recv_buffer_size = tcp_in
        .as_ref()
        .recv_buffer_size()
        .context("Failed getting SO_RCVBUF")?;
    let mut tcp_in = BufReader::with_capacity(tcp_recv_buffer_size, tcp_in);
    let mut buffer = [0u8; MAX_DATAGRAM_SIZE];
    loop {
        let datagram_len = tcp_in.read_u16().await? as usize;
        let tcp_read_len = tcp_in
            .read_exact(&mut buffer[..datagram_len])
            .await
            .context("Failed reading from TCP")?;
        let udp_write_len = udp_out
            .send(&buffer[..datagram_len])
            .await
            .context("Failed writing to UDP")?;
        if tcp_read_len != udp_write_len {
            log::warn!(
                "Read {} bytes from TCP but wrote only {} to UDP",
                tcp_read_len,
                udp_write_len
            );
        } else {
            log::trace!("Forwarded {} bytes TCP->UDP", tcp_read_len);
        }
    }
}

async fn process_udp2tcp(
    mut udp_in: UdpRecvHalf,
    mut tcp_out: TcpWriteHalf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0u8; 2 + MAX_DATAGRAM_SIZE];
    loop {
        let udp_read_len = udp_in
            .recv(&mut buffer[2..])
            .await
            .context("Failed reading from UDP")?;
        if udp_read_len == 0 {
            break;
        }
        let datagram_len = u16::try_from(udp_read_len).unwrap();
        buffer[..2].copy_from_slice(&datagram_len.to_be_bytes()[..]);
        tcp_out
            .write_all(&buffer[..2 + udp_read_len])
            .await
            .context("Failed writing to TCP")?;
        log::trace!("Forwarded {} bytes UDP->TCP", udp_read_len);
    }
    Ok(())
}
