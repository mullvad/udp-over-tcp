use err_context::ResultExt as _;
use std::convert::TryFrom;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf as TcpReadHalf, OwnedWriteHalf as TcpWriteHalf};
use tokio::net::udp::{RecvHalf as UdpRecvHalf, SendHalf as UdpSendHalf};

pub async fn process_tcp2udp(
    mut tcp_in: TcpReadHalf,
    mut udp_out: UdpSendHalf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0u8; 1024 * 64];
    loop {
        let datagram_len = tcp_in.read_u16().await? as usize;
        let tcp_read_len = tcp_in
            .read_exact(&mut buffer[..datagram_len])
            .await
            .context("Failed reading from TCP")?;
        assert_eq!(datagram_len, tcp_read_len);
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

pub async fn process_udp2tcp(
    mut udp_in: UdpRecvHalf,
    mut tcp_out: TcpWriteHalf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0u8; 2 + 1024 * 64];
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
