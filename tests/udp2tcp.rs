use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use udp_over_tcp::{udp2tcp, TcpOptions};

/// Set everything up and then close the TCP socket. That should cleanly make the Udp2Tcp
/// instance shut down.
#[tokio::test]
async fn connect_close_tcp_socket() -> Result<(), Box<dyn std::error::Error>> {
    let (udp_socket, mut tcp_stream, udp2tcp) = setup_udp2tcp().await?;

    tcp_stream.shutdown().await?;

    // Send empty datagram to make it connect/activate
    udp_socket.send(&[]).await?;

    udp2tcp
        .await?
        .expect("Clean shutdown of TCP socket should cleanly shut down Udp2Tcp");
    Ok(())
}

/// Set everything up and then close the Udp2Tcp. This should close the TCP socket.
#[tokio::test]
async fn connect_close_tcp2udp() -> Result<(), Box<dyn std::error::Error>> {
    let (_udp_socket, mut tcp_stream, udp2tcp) = setup_udp2tcp().await?;

    udp2tcp.abort();

    let read_len = tcp_stream.read(&mut [0u8; 1024]).await?;
    assert_eq!(read_len, 0);

    Ok(())
}

/// Send one datagram UDP->TCP and one back TCP->UDP
/// We can't test only TCP->UDP since a datagram must be sent
/// on the UDP socket before the Udp2Tcp instance even starts
/// working
#[tokio::test]
async fn ping_pong() -> Result<(), Box<dyn std::error::Error>> {
    let (udp_socket, mut tcp_stream, _) = setup_udp2tcp().await?;
    let mut buffer = [0; 1024];

    // Send a datagram into the udp2tcp forwarder and assert
    // it comes out the TCP socket with the correct format.
    {
        let write_len = dbg!(udp_socket.send(&[1, 2, 3]).await?);
        assert_eq!(write_len, 3);

        let read_len = tcp_stream.read(&mut buffer[..]).await?;
        assert_eq!(read_len, 5);
        // Big endian header
        assert_eq!(&buffer[0..2], &[0, 3]);
        // Datagram body
        assert_eq!(&buffer[2..5], &[1, 2, 3]);
    }

    // Send an encoded datagram back down the TCP socket and assert
    // it comes out correctly formatted on the UDP socket.
    {
        // Write big endian header
        tcp_stream.write_all(&[0u8, 2]).await?;
        // Write datagram body
        tcp_stream.write_all(&[9, 8]).await?;

        let mut buffer = [0; 1024];
        let read_len = dbg!(udp_socket.recv(&mut buffer[..]).await?);
        assert_eq!(read_len, 2);
        assert_eq!(&buffer[..2], &[9, 8]);
    }
    Ok(())
}

/// Send two encoded datagrams down the TCP socket but in chunks
/// and assert they come out correctly formatted on the UDP socket.
#[tokio::test]
async fn pong_two_separate() -> Result<(), Box<dyn std::error::Error>> {
    let (udp_socket, mut tcp_stream, _) = setup_udp2tcp().await?;
    let mut buffer = [0; 1024];

    // Send empty datagram to make it connect/activate
    udp_socket.send(&[]).await?;

    // Send first datagram and part of the second
    #[rustfmt::skip]
    tcp_stream
        .write_all(&[
            /* header */ 0, 2, /* body */ 1, 2,
            /* header */ 0, 5, /* partial body */ 9, 8,
        ])
        .await?;

    // Read out first datagram
    {
        let read_len = dbg!(udp_socket.recv(&mut buffer[..]).await?);
        assert_eq!(read_len, 2);
        assert_eq!(&buffer[..2], &[1, 2]);
    }

    // Write remaining datagram body
    tcp_stream.write_all(&[7, 6, 5]).await?;

    // Read out second datagram
    {
        let read_len = dbg!(udp_socket.recv(&mut buffer[..]).await?);
        assert_eq!(read_len, 5);
        assert_eq!(&buffer[..5], &[9, 8, 7, 6, 5]);
    }

    Ok(())
}

/// Spawns a Udp2Tcp instance and connects to boths ends of it.
/// Returns the UDP and TCP sockets that goes through it.
async fn setup_udp2tcp() -> Result<
    (UdpSocket, TcpStream, JoinHandle<Result<(), udp2tcp::Error>>),
    Box<dyn std::error::Error>,
> {
    let tcp_listener = TcpListener::bind("127.0.0.1:0").await?;
    let tcp_listen_addr = tcp_listener.local_addr().unwrap();

    let udp2tcp = udp2tcp::Udp2Tcp::new(
        "127.0.0.1:0".parse().unwrap(),
        tcp_listen_addr,
        TcpOptions::default(),
    )
    .await?;

    let udp_listen_addr = udp2tcp.local_udp_addr().unwrap();
    let join_handle = tokio::spawn(udp2tcp.run());

    let udp_socket = UdpSocket::bind("127.0.0.1:0").await?;
    udp_socket.connect(udp_listen_addr).await?;

    // Send empty datagram to connect TCP socket
    udp_socket.send(&[]).await?;
    let mut tcp_stream = tcp_listener.accept().await?.0;
    let mut buf = [0; 1024];
    tcp_stream.read(&mut buf).await?;

    Ok((udp_socket, tcp_stream, join_handle))
}
