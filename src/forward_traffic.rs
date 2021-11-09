use crate::NeverOkResult;
use err_context::BoxedErrorExt as _;
use err_context::ResultExt as _;
use futures::future::{abortable, select, Either};
use std::convert::{Infallible, TryFrom};
use std::io;
use std::mem;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf as TcpReadHalf, OwnedWriteHalf as TcpWriteHalf};
use tokio::net::{TcpStream, UdpSocket};

/// A UDP datagram header has a 16 bit field containing an unsigned integer
/// describing the length of the datagram (including the header itself).
/// The max value is 2^16 = 65534 bytes. But since that includes the
/// UDP header, this constant is 8 bytes more than any UDP socket
/// read operation would ever return. We are going to use that extra space
/// to store our 2 byte udp-over-tcp header.
pub const MAX_DATAGRAM_SIZE: usize = u16::MAX as usize;
const HEADER_LEN: usize = mem::size_of::<u16>();

/// Forward traffic between the given UDP and TCP sockets in both directions.
/// This async function runs until one of the sockets are closed or there is an error.
/// Both sockets are closed before returning.
pub async fn process_udp_over_tcp(udp_socket: UdpSocket, tcp_stream: TcpStream) {
    let udp_in = Arc::new(udp_socket);
    let udp_out = udp_in.clone();
    let (tcp_in, tcp_out) = tcp_stream.into_split();

    let (tcp2udp_future, tcp2udp_abort) = abortable(async move {
        if let Err(error) = process_tcp2udp(tcp_in, udp_out).await {
            log::error!("Error: {}", error.display("\nCaused by: "));
        }
    });
    let (udp2tcp_future, udp2tcp_abort) = abortable(async move {
        let error = process_udp2tcp(udp_in, tcp_out).await.into_err();
        log::error!("Error: {}", error.display("\nCaused by: "));
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

/// Reads from `tcp_in` and extracts UDP datagrams. Writes the datagrams to `udp_out`.
/// Returns if the TCP socket is closed, or an IO error happens on either socket.
async fn process_tcp2udp(
    mut tcp_in: TcpReadHalf,
    udp_out: Arc<UdpSocket>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = datagram_buffer();
    // `buffer` has unprocessed data from the TCP socket up until this index.
    let mut unprocessed_i = 0;
    loop {
        let tcp_read_len = tcp_in
            .read(&mut buffer[unprocessed_i..])
            .await
            .context("Failed reading from TCP")?;
        if tcp_read_len == 0 {
            break;
        }
        unprocessed_i += tcp_read_len;

        let processed_i = forward_datagrams_in_buffer(&udp_out, &buffer[..unprocessed_i])
            .await
            .context("Failed writing to UDP")?;

        // If we have read data that was not forwarded, because it was not a complete datagram,
        // move it to the start of the buffer and start over
        if unprocessed_i > processed_i {
            buffer.copy_within(processed_i..unprocessed_i, 0);
        }
        unprocessed_i -= processed_i;
    }
    log::debug!("TCP socket closed");
    Ok(())
}

/// Forward all complete datagrams in `buffer` to `udp_out`.
/// Returns the number of processed bytes.
async fn forward_datagrams_in_buffer(udp_out: &UdpSocket, buffer: &[u8]) -> io::Result<usize> {
    let mut header_start = 0;
    loop {
        let header_end = header_start + HEADER_LEN;
        // "parse" the header
        let header = match buffer.get(header_start..header_end) {
            Some(header) => <[u8; HEADER_LEN]>::try_from(header).unwrap(),
            // Buffer does not contain entire header for next datagram
            None => break Ok(header_start),
        };
        let datagram_len = usize::from(u16::from_be_bytes(header));
        let datagram_start = header_end;
        let datagram_end = datagram_start + datagram_len;

        let datagram_data = match buffer.get(datagram_start..datagram_end) {
            Some(datagram_data) => datagram_data,
            // The buffer does not contain the entire datagram
            None => break Ok(header_start),
        };

        let udp_write_len = udp_out.send(datagram_data).await?;
        assert_eq!(
            udp_write_len, datagram_len,
            "Did not send entire UDP datagram"
        );
        log::trace!("Forwarded {} byte TCP->UDP", datagram_len);

        header_start = datagram_end;
    }
}

/// Reads datagrams from `udp_in` and writes them (with the 16 bit header containing the length)
/// to `tcp_out` indefinitely, or until an IO error happens on either socket.
async fn process_udp2tcp(
    udp_in: Arc<UdpSocket>,
    mut tcp_out: TcpWriteHalf,
) -> Result<Infallible, Box<dyn std::error::Error>> {
    // A buffer large enough to hold any possible UDP datagram plus its 16 bit length header.
    let mut buffer = datagram_buffer();
    loop {
        let udp_read_len = udp_in
            .recv(&mut buffer[HEADER_LEN..])
            .await
            .context("Failed reading from UDP")?;

        // Set the "header" to the length of the datagram.
        let datagram_len =
            u16::try_from(udp_read_len).expect("UDP datagram can't be larger than 2^16");
        buffer[..HEADER_LEN].copy_from_slice(&datagram_len.to_be_bytes()[..]);

        tcp_out
            .write_all(&buffer[..HEADER_LEN + udp_read_len])
            .await
            .context("Failed writing to TCP")?;

        log::trace!("Forwarded {} bytes UDP->TCP", udp_read_len);
    }
}

/// Creates and returns a buffer on the heap with enough space to contain any possible
/// UDP datagram.
///
/// This is put on the heap and in a separate function to avoid the 64k buffer from ending
/// up on the stack and blowing up the size of the futures using it.
#[inline(never)]
pub fn datagram_buffer() -> Box<[u8; MAX_DATAGRAM_SIZE]> {
    Box::new([0u8; MAX_DATAGRAM_SIZE])
}
