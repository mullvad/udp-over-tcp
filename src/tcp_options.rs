use std::fmt;
use std::io;
use tokio::net::TcpSocket;

/// Options to apply to the TCP socket involved in the tunneling.
#[derive(Debug, Default, Clone, structopt::StructOpt)]
pub struct TcpOptions {
    /// If given, sets the SO_RCVBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's receive buffer associated with the socket.
    #[structopt(long = "recv-buffer")]
    pub recv_buffer_size: Option<u32>,

    /// If given, sets the SO_SNDBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's send buffer associated with the socket.
    #[structopt(long = "send-buffer")]
    pub send_buffer_size: Option<u32>,
}

#[derive(Debug)]
pub enum ApplyTcpOptionsError {
    /// Failed to get/set TCP_RCVBUF
    RecvBuffer(io::Error),

    /// Failed to get/set TCP_SNDBUF
    SendBuffer(io::Error),
}

impl fmt::Display for ApplyTcpOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ApplyTcpOptionsError::*;
        match self {
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
            RecvBuffer(e) => Some(e),
            SendBuffer(e) => Some(e),
        }
    }
}

/// Applies the given options to the given TCP socket.
pub fn apply(socket: &TcpSocket, options: &TcpOptions) -> Result<(), ApplyTcpOptionsError> {
    if let Some(recv_buffer_size) = options.recv_buffer_size {
        socket
            .set_recv_buffer_size(recv_buffer_size)
            .map_err(ApplyTcpOptionsError::RecvBuffer)?;
    }
    log::debug!(
        "SO_RCVBUF: {}",
        socket
            .recv_buffer_size()
            .map_err(ApplyTcpOptionsError::RecvBuffer)?
    );
    if let Some(send_buffer_size) = options.send_buffer_size {
        socket
            .set_send_buffer_size(send_buffer_size)
            .map_err(ApplyTcpOptionsError::SendBuffer)?;
    }
    log::debug!(
        "SO_SNDBUF: {}",
        socket
            .send_buffer_size()
            .map_err(ApplyTcpOptionsError::SendBuffer)?
    );
    Ok(())
}
