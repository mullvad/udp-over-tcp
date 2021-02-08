use err_context::BoxedErrorExt as _;
use err_context::ResultExt as _;
use futures::future::{abortable, select, Either};
use std::convert::TryFrom;
use std::mem;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf as TcpReadHalf, OwnedWriteHalf as TcpWriteHalf};
use tokio::net::{TcpStream, UdpSocket};

const MAX_DATAGRAM_SIZE: usize = u16::MAX as usize;
const HEADER_LEN: usize = mem::size_of::<u16>();

type Error = err_context::Context<&'static str, std::io::Error>;

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
    mut tcp_in: TcpReadHalf,
    udp_out: Arc<UdpSocket>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0u8; MAX_DATAGRAM_SIZE];
    let mut tcp_read_len = 0;
    loop {
        log::trace!("Beginning processing round with tcp_read_len={}", tcp_read_len);
        // Read at least the entire header
        if tcp_read_len < HEADER_LEN {
            tcp_read_len += read_at_least(
                &mut tcp_in,
                &mut buffer[tcp_read_len..],
                HEADER_LEN - tcp_read_len,
            )
            .await?;
            if tcp_read_len < HEADER_LEN {
                break;
            }
        }
        // "parse" the header
        let header = <[u8; HEADER_LEN]>::try_from(&buffer[..HEADER_LEN]).unwrap();
        let datagram0_len = usize::from(u16::from_be_bytes(header));
        let datagram0_end = HEADER_LEN + datagram0_len;

        // Read at least the entire datagram after the header
        if tcp_read_len < datagram0_end {
            let remaining_len = datagram0_end - tcp_read_len;
            tcp_read_len +=
                read_at_least(&mut tcp_in, &mut buffer[tcp_read_len..], remaining_len).await?;
        }
        if tcp_read_len < datagram0_end {
            break;
        }

        let consumed_len = forward_datagrams_in_buffer(&udp_out, &buffer[..tcp_read_len]).await?;

        // If we have read data that was not forwarded, because it was not a complete datagram,
        // move it to the start of the buffer and start over
        if tcp_read_len > consumed_len {
            let bytes_left = tcp_read_len - consumed_len;
            log::warn!(
                "Copying {}..{} (={}) remaining bytes to start of buffer",
                consumed_len,
                tcp_read_len,
                bytes_left
            );
            buffer.copy_within(consumed_len..tcp_read_len, 0);
            tcp_read_len -= consumed_len;
        } else {
            tcp_read_len = 0;
        }
    }
    log::info!("TCP socket closed");
    Ok(())
}

async fn forward_datagrams_in_buffer(
    udp_out: &UdpSocket,
    buf: &[u8],
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut header_start = 0;
    let mut datagram_count = 0;
    loop {
        let header_end = header_start + HEADER_LEN;
        if header_end > buf.len() {
            // Buffer does not contain entire header for next datagram
            log::trace!(
                "Not enough data to read header on datagram {}",
                datagram_count
            );
            break Ok(header_start);
        }
        // "parse" the header
        let header = <[u8; HEADER_LEN]>::try_from(&buf[header_start..header_end]).unwrap();
        let datagram_len = usize::from(u16::from_be_bytes(header));

        let datagram_start = header_end;
        let datagram_end = datagram_start + datagram_len;

        if datagram_end > buf.len() {
            // The buffer does not contain the entire datagram
            log::trace!("Not enough data for body of datagram {}", datagram_count);
            break Ok(header_start);
        }

        let udp_write_len = udp_out
            .send(&buf[datagram_start..datagram_end])
            .await
            .context("Failed writing to UDP")?;
        assert_eq!(
            udp_write_len, datagram_len,
            "Did not send entire UDP datagram"
        );
        log::trace!(
            "Forwarding {} byte TCP->UDP {}..{} (datagram #{})",
            datagram_len,
            datagram_start,
            datagram_end,
            datagram_count,
        );

        header_start = datagram_end;
        datagram_count += 1;
    }
}

async fn read_at_least(
    tcp_in: &mut TcpReadHalf,
    buf: &mut [u8],
    len: usize,
) -> Result<usize, Error> {
    assert!(len < buf.len());
    let mut bytes_read = 0;
    while bytes_read < len {
        let tcp_read_len = tcp_in
            .read(&mut buf[bytes_read..])
            .await
            .context("Failed reading from TCP")?;
        if tcp_read_len == 0 {
            // EOF
            log::trace!("TCP EOF");
            break;
        }
        bytes_read += tcp_read_len;
        log::trace!("TCP read {}/{} bytes", bytes_read, len);
    }
    Ok(bytes_read)
}

async fn process_udp2tcp(
    udp_in: Arc<UdpSocket>,
    mut tcp_out: TcpWriteHalf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0u8; MAX_DATAGRAM_SIZE];
    loop {
        let udp_read_len = udp_in
            .recv(&mut buffer[HEADER_LEN..])
            .await
            .context("Failed reading from UDP")?;
        if udp_read_len == 0 {
            log::info!("UDP socket closed");
            break;
        }

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
    Ok(())
}
