#[cfg(target_os = "linux")]
use nix::sys::socket::{getsockopt, setsockopt, sockopt};
use std::fmt;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use tokio::net::TcpSocket;

/// Options to apply to the TCP socket involved in the tunneling.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct TcpOptions {
    /// If given, sets the SO_RCVBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's receive buffer associated with the socket.
    #[cfg_attr(feature = "clap", arg(long = "recv-buffer"))]
    pub recv_buffer_size: Option<u32>,

    /// If given, sets the SO_SNDBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's send buffer associated with the socket.
    #[cfg_attr(feature = "clap", arg(long = "send-buffer"))]
    pub send_buffer_size: Option<u32>,

    /// An application timeout on receiving data from the TCP socket.
    #[cfg_attr(feature = "clap", arg(long = "tcp-recv-timeout", value_parser = duration_secs_from_str))]
    pub recv_timeout: Option<Duration>,

    /// If given, sets the SO_MARK option on the TCP socket.
    /// This exists only on Linux.
    #[cfg(target_os = "linux")]
    #[cfg_attr(feature = "clap", arg(long = "fwmark"))]
    pub fwmark: Option<u32>,

    /// Enables TCP_NODELAY on the TCP socket.
    /// This exists only on Linux for now.
    #[cfg(target_os = "linux")]
    #[cfg_attr(feature = "clap", arg(long))]
    pub nodelay: bool,
}

#[derive(Debug)]
pub enum ApplyTcpOptionsError {
    /// Failed to get/set TCP_RCVBUF
    RecvBuffer(io::Error),

    /// Failed to get/set TCP_SNDBUF
    SendBuffer(io::Error),

    /// Failed to get/set SO_MARK
    #[cfg(target_os = "linux")]
    Mark(nix::Error),

    /// Failed to get/set TCP_NODELAY
    #[cfg(target_os = "linux")]
    TcpNoDelay(nix::Error),
}

impl fmt::Display for ApplyTcpOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ApplyTcpOptionsError::*;
        match self {
            RecvBuffer(_) => "Failed to get/set TCP_RCVBUF",
            SendBuffer(_) => "Failed to get/set TCP_SNDBUF",
            #[cfg(target_os = "linux")]
            Mark(_) => "Failed to get/set SO_MARK",
            #[cfg(target_os = "linux")]
            TcpNoDelay(_) => "Failed to get/set TCP_NODELAY",
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
            #[cfg(target_os = "linux")]
            Mark(e) => Some(e),
            #[cfg(target_os = "linux")]
            TcpNoDelay(e) => Some(e),
        }
    }
}

#[cfg(feature = "clap")]
fn duration_secs_from_str(str_duration: &str) -> Result<Duration, std::num::ParseIntError> {
    use std::str::FromStr;
    u64::from_str(str_duration).map(Duration::from_secs)
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
    #[cfg(target_os = "linux")]
    {
        let fd = socket.as_raw_fd();
        if let Some(fwmark) = options.fwmark {
            setsockopt(fd, sockopt::Mark, &fwmark).map_err(ApplyTcpOptionsError::Mark)?;
        }
        log::debug!(
            "SO_MARK: {}",
            getsockopt(fd, sockopt::Mark).map_err(ApplyTcpOptionsError::Mark)?
        );
    }
    #[cfg(target_os = "linux")]
    {
        let fd = socket.as_raw_fd();
        if options.nodelay {
            setsockopt(fd, sockopt::TcpNoDelay, &true).map_err(ApplyTcpOptionsError::TcpNoDelay)?;
        }
        log::debug!(
            "TCP_NODELAY: {}",
            getsockopt(fd, sockopt::TcpNoDelay).map_err(ApplyTcpOptionsError::TcpNoDelay)?
        );
    }
    Ok(())
}
