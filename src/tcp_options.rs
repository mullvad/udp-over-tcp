#[cfg(target_os = "linux")]
use nix::sys::socket::{getsockopt, setsockopt, sockopt};
use std::fmt;
use std::time::Duration;
use tokio::net::{TcpSocket, TcpStream};

/// Options to apply to the TCP socket involved in the tunneling.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
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
    #[cfg_attr(feature = "clap", arg(long))]
    pub nodelay: bool,
}

/// Represents a failure to apply socket options to the TCP socket
#[derive(Debug)]
pub struct ApplyTcpOptionsError {
    source: Box<dyn std::error::Error + Send + 'static>,
    kind: ApplyTcpOptionsErrorKind,
}

#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub enum ApplyTcpOptionsErrorKind {
    /// Failed to get/set TCP_RCVBUF
    RecvBuffer,

    /// Failed to get/set TCP_SNDBUF
    SendBuffer,

    /// Failed to get/set SO_MARK
    #[cfg(target_os = "linux")]
    Mark,

    /// Failed to get/set TCP_NODELAY
    TcpNoDelay,
}

impl ApplyTcpOptionsError {
    fn new(
        kind: ApplyTcpOptionsErrorKind,
        source: impl std::error::Error + Send + 'static,
    ) -> Self {
        Self {
            source: Box::new(source) as Box<dyn std::error::Error + Send + 'static>,
            kind: kind,
        }
    }
}

impl fmt::Display for ApplyTcpOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ApplyTcpOptionsErrorKind::*;
        match self.kind {
            RecvBuffer => "Failed to get/set TCP_RCVBUF",
            SendBuffer => "Failed to get/set TCP_SNDBUF",
            #[cfg(target_os = "linux")]
            Mark => "Failed to get/set SO_MARK",
            TcpNoDelay => "Failed to get/set TCP_NODELAY",
        }
        .fmt(f)
    }
}

impl std::error::Error for ApplyTcpOptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
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
            .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::RecvBuffer, e))?;
    }
    log::debug!(
        "SO_RCVBUF: {}",
        socket
            .recv_buffer_size()
            .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::RecvBuffer, e))?
    );
    if let Some(send_buffer_size) = options.send_buffer_size {
        socket
            .set_send_buffer_size(send_buffer_size)
            .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::SendBuffer, e))?;
    }
    log::debug!(
        "SO_SNDBUF: {}",
        socket
            .send_buffer_size()
            .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::SendBuffer, e))?
    );
    #[cfg(target_os = "linux")]
    {
        if let Some(fwmark) = options.fwmark {
            setsockopt(&socket, sockopt::Mark, &fwmark)
                .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::Mark, e))?;
        }
        log::debug!(
            "SO_MARK: {}",
            getsockopt(&socket, sockopt::Mark)
                .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::Mark, e))?
        );
    }
    Ok(())
}

/// We need to apply the nodelay option separately as it is not currently exposed on TcpSocket.
/// => https://github.com/tokio-rs/tokio/issues/5510
pub fn set_nodelay(tcp_stream: &TcpStream, nodelay: bool) -> Result<(), ApplyTcpOptionsError> {
    // Configure TCP_NODELAY on the TCP stream
    tcp_stream
        .set_nodelay(nodelay)
        .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::TcpNoDelay, e))?;
    log::debug!(
        "TCP_NODELAY: {}",
        tcp_stream
            .nodelay()
            .map_err(|e| ApplyTcpOptionsError::new(ApplyTcpOptionsErrorKind::TcpNoDelay, e))?
    );
    Ok(())
}
