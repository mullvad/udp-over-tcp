//! Tests that [`process_udp_over_tcp`] can tunnel datagrams from something else
//! than an actual UDP socket.

use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use udp_over_tcp::{DatagramSocket, process_udp_over_tcp};

/// An in-memory datagram transport. Datagrams sent by the forwarder end up in `outgoing`,
/// and datagrams pushed into `incoming` are received by the forwarder.
struct ChannelSocket {
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
    incoming: Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl DatagramSocket for ChannelSocket {
    async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.outgoing
            .send(buf.to_vec())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut incoming = self.incoming.lock().await;
        let datagram = incoming
            .recv()
            .await
            .ok_or(io::Error::from(io::ErrorKind::BrokenPipe))?;
        buf[..datagram.len()].copy_from_slice(&datagram);
        Ok(datagram.len())
    }
}

/// Send one datagram transport->TCP and one back TCP->transport.
#[tokio::test]
async fn ping_pong() -> Result<(), Box<dyn std::error::Error>> {
    let (to_forwarder, mut from_forwarder, mut tcp_stream) = setup().await?;
    let mut buffer = [0; 1024];

    // Send a datagram into the forwarder and assert it comes out
    // the TCP socket with the correct format.
    {
        to_forwarder.send(vec![1, 2, 3])?;

        let read_len = tcp_stream.read(&mut buffer[..]).await?;
        assert_eq!(read_len, 5);
        // Big endian header
        assert_eq!(&buffer[0..2], &[0, 3]);
        // Datagram body
        assert_eq!(&buffer[2..5], &[1, 2, 3]);
    }

    // Send an encoded datagram down the TCP socket and assert it comes
    // back out of the transport as a single datagram.
    {
        // Write big endian header
        tcp_stream.write_all(&[0u8, 2]).await?;
        // Write datagram body
        tcp_stream.write_all(&[9, 8]).await?;

        assert_eq!(from_forwarder.recv().await, Some(vec![9, 8]));
    }
    Ok(())
}

/// Spawns a forwarder between a [`ChannelSocket`] and a TCP socket. Returns the two
/// ends of the transport plus the other end of the TCP connection.
async fn setup() -> Result<
    (
        mpsc::UnboundedSender<Vec<u8>>,
        mpsc::UnboundedReceiver<Vec<u8>>,
        TcpStream,
    ),
    Box<dyn std::error::Error>,
> {
    let tcp_listener = TcpListener::bind("127.0.0.1:0").await?;
    let tcp_stream = TcpStream::connect(tcp_listener.local_addr()?).await?;
    let forwarder_tcp_stream = tcp_listener.accept().await?.0;

    let (to_forwarder, incoming) = mpsc::unbounded_channel();
    let (outgoing, from_forwarder) = mpsc::unbounded_channel();
    let socket = ChannelSocket {
        outgoing,
        incoming: Mutex::new(incoming),
    };

    // Spawning also asserts that the forwarder future is `Send`.
    tokio::spawn(process_udp_over_tcp(socket, forwarder_tcp_stream, None));

    Ok((to_forwarder, from_forwarder, tcp_stream))
}
