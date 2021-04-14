use std::fmt;

lazy_static::lazy_static! {
    /// If true, redact IPs and other user sensitive data from logs
    static ref REDACT_LOGS: bool = std::env::var("REDACT_LOGS")
        .map(|v| v != "0")
        .unwrap_or(false);
}

/// Wrap any displayable type in this to have its Display/Debug format be redacted at runtime
/// if the user so wish. Makes it possible to log more extensively without collecting user
/// sensitivie data.
pub struct Redact<T>(pub T);

impl<T: fmt::Display> fmt::Display for Redact<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *REDACT_LOGS {
            true => write!(f, "[REDACTED]"),
            false => self.0.fmt(f),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Redact<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *REDACT_LOGS {
            true => write!(f, "[REDACTED]"),
            false => self.0.fmt(f),
        }
    }
}
