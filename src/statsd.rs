#[cfg(feature = "statsd")]
pub use real::Error;

pub struct StatsdMetrics(StatsdMetricsChooser);

enum StatsdMetricsChooser {
    Dummy,
    #[cfg(feature = "statsd")]
    Real(real::StatsdMetrics),
}

impl StatsdMetrics {
    /// Creates a dummy statsd metrics instance. Does not actually connect to any statds
    /// server, nor emits any events. Used as an API compatible drop in when metrics
    /// should not be emitted.
    pub fn dummy() -> Self {
        Self(StatsdMetricsChooser::Dummy)
    }

    /// Creates a statsd metric reporting instance connecting to the given host addr.
    #[cfg(feature = "statsd")]
    pub fn real(host: std::net::SocketAddr) -> Result<Self, Error> {
        let statsd = real::StatsdMetrics::new(host)?;
        Ok(Self(StatsdMetricsChooser::Real(statsd)))
    }

    /// Emit a metric saying we failed to accept an incoming TCP connection (probably ran out of file descriptors)
    pub fn accept_error(&self) {
        #[cfg(feature = "statsd")]
        if let StatsdMetricsChooser::Real(statsd) = &self.0 {
            statsd.accept_error()
        }
    }

    /// Increment the connection counter inside this metrics instance and emit that new gauge value
    pub fn incr_connections(&self) {
        #[cfg(feature = "statsd")]
        if let StatsdMetricsChooser::Real(statsd) = &self.0 {
            statsd.incr_connections()
        }
    }

    /// Decrement the connection counter inside this metrics instance and emit that new gauge value
    pub fn decr_connections(&self) {
        #[cfg(feature = "statsd")]
        if let StatsdMetricsChooser::Real(statsd) = &self.0 {
            statsd.decr_connections()
        }
    }
}

#[cfg(feature = "statsd")]
mod real {
    use cadence::{CountedExt, Gauged, QueuingMetricSink, StatsdClient, UdpMetricSink};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Queue with a maximum capacity of 8K events.
    /// This program is extremely unlikely to ever reach that upper bound.
    /// The bound is still here so that if it ever were to happen, we drop events
    /// instead of indefinitely filling the memory with unsent events.
    const QUEUE_SIZE: usize = 8 * 1024;

    const PREFIX: &str = "tcp2udp";

    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Error {
        /// Failed to create + bind the statsd UDP socket.
        BindUdpSocket(std::io::Error),
        /// Failed to create statsd client.
        CreateStatsdClient(cadence::MetricError),
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            use Error::*;
            match self {
                BindUdpSocket(_) => "Failed to bind the UDP socket".fmt(f),
                CreateStatsdClient(e) => e.fmt(f),
            }
        }
    }

    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            use Error::*;
            match self {
                BindUdpSocket(e) => Some(e),
                CreateStatsdClient(e) => e.source(),
            }
        }
    }

    pub struct StatsdMetrics {
        client: StatsdClient,
        num_connections: AtomicU64,
    }

    impl StatsdMetrics {
        pub fn new(host: std::net::SocketAddr) -> Result<Self, Error> {
            let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(Error::BindUdpSocket)?;
            log::debug!(
                "Statsd socket bound to {}",
                socket
                    .local_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "Unknown".to_owned())
            );

            // Create a non-buffered blocking metrics sink. It is important that it's not buffered,
            // so events are emitted instantly when they happen (this program does not emit a lot of
            // events, nor does it attach timestamps to the events.
            // The fact that it's blocking does not matter, since the `QueuingMetricSink` will make sure
            // the `UdpMetricSink` runs in its own thread anyway.
            let udp_sink = UdpMetricSink::from(host, socket).map_err(Error::CreateStatsdClient)?;
            let queuing_sink = QueuingMetricSink::with_capacity(udp_sink, QUEUE_SIZE);
            let statds_client = StatsdClient::from_sink(PREFIX, queuing_sink);
            Ok(Self {
                client: statds_client,
                num_connections: AtomicU64::new(0),
            })
        }

        pub fn accept_error(&self) {
            log::debug!("Sending statsd tcp_accept_errors");
            if let Err(e) = self.client.incr("tcp_accept_errors") {
                log::error!("Failed to emit statsd tcp_accept_errors: {e}");
            }
        }

        pub fn incr_connections(&self) {
            let num_connections = self.num_connections.fetch_add(1, Ordering::Relaxed) + 1;
            log::debug!("Sending statsd num_connections = {num_connections}");
            if let Err(e) = self.client.gauge("num_connections", num_connections) {
                log::error!("Failed to emit statsd num_connections: {e}");
            }
        }

        pub fn decr_connections(&self) {
            let num_connections = self.num_connections.fetch_sub(1, Ordering::Relaxed) - 1;
            log::debug!("Sending statsd num_connections = {num_connections}");
            if let Err(e) = self.client.gauge("num_connections", num_connections) {
                log::error!("Failed to emit statsd num_connections: {e}");
            }
        }
    }
}
