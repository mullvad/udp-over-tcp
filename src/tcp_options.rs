use std::fmt;
use std::io;
use tokio::net::TcpStream;

/// Options to apply to the TCP socket involved in the tunneling.
#[derive(Debug, structopt::StructOpt)]
pub struct TcpOptions {
    /// Sets the TCP_NODELAY option on the TCP socket.
    /// If set, this option disables the Nagle algorithm.
    /// This means that segments are always sent as soon as possible.
    #[structopt(long = "nodelay")]
    pub nodelay: bool,

    /// If given, sets the SO_RCVBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's receive buffer associated with the socket.
    #[structopt(long = "recv-buffer")]
    pub recv_buffer_size: Option<usize>,

    /// If given, sets the SO_SNDBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's send buffer associated with the socket.
    #[structopt(long = "send-buffer")]
    pub send_buffer_size: Option<usize>,
}

#[derive(Debug)]
pub enum ApplyTcpOptionsError {
    NoDelay(io::Error),
    RecvBuffer(io::Error),
    SendBuffer(io::Error),
}

impl fmt::Display for ApplyTcpOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ApplyTcpOptionsError::*;
        match self {
            NoDelay(_) => "Failed to get/set TCP_NODELAY",
            RecvBuffer(_) => "Failed to get/set TCP_RCVBUF",
            SendBuffer(_) => "Failed to get/set TCP_SNDBUF",
        }
        .fmt(f)
    }
}

impl std::error::Error for ApplyTcpOptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use ApplyTcpOptionsError::*;
        match self {
            NoDelay(e) => Some(e),
            RecvBuffer(e) => Some(e),
            SendBuffer(e) => Some(e),
        }
    }
}

/// Applies the given options to the given TCP socket.
pub fn apply(tcp_stream: &TcpStream, options: &TcpOptions) -> Result<(), ApplyTcpOptionsError> {
    if options.nodelay {
        tcp_stream
            .set_nodelay(true)
            .map_err(ApplyTcpOptionsError::NoDelay)?;
    }
    log::debug!(
        "TCP_NODELAY: {}",
        tcp_stream
            .nodelay()
            .map_err(ApplyTcpOptionsError::NoDelay)?
    );
    if let Some(recv_buffer_size) = options.recv_buffer_size {
        tcp_stream
            .set_recv_buffer_size(recv_buffer_size)
            .map_err(ApplyTcpOptionsError::RecvBuffer)?;
    }
    log::debug!(
        "SO_RCVBUF: {}",
        tcp_stream
            .recv_buffer_size()
            .map_err(ApplyTcpOptionsError::RecvBuffer)?
    );
    if let Some(send_buffer_size) = options.send_buffer_size {
        tcp_stream
            .set_send_buffer_size(send_buffer_size)
            .map_err(ApplyTcpOptionsError::SendBuffer)?;
    }
    log::debug!(
        "SO_SNDBUF: {}",
        tcp_stream
            .send_buffer_size()
            .map_err(ApplyTcpOptionsError::SendBuffer)?
    );
    Ok(())
}
