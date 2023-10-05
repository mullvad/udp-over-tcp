use std::cmp;
use std::time::Duration;

/// Simple exponential backoff
pub struct ExponentialBackoff {
    start_delay: Duration,
    max_delay: Duration,
    current_delay: Duration,
}

impl ExponentialBackoff {
    /// Creates a new exponential backoff instance starting with delay
    /// `start_delay` and maxing out at `max_delay`.
    pub fn new(start_delay: Duration, max_delay: Duration) -> Self {
        Self {
            start_delay,
            max_delay,
            current_delay: start_delay,
        }
    }

    /// Resets the exponential backoff so that the next delay is the start delay again.
    pub fn reset(&mut self) {
        self.current_delay = self.start_delay;
    }

    /// Returns the next delay. This is twice as long as the last returned delay,
    /// up until `max_delay` is reached.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current_delay;

        let next_delay = self.current_delay * 2;
        self.current_delay = cmp::min(next_delay, self.max_delay);

        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_delays() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(60), Duration::from_millis(300));
        assert_eq!(backoff.next_delay(), Duration::from_millis(60));
        assert_eq!(backoff.next_delay(), Duration::from_millis(120));
        assert_eq!(backoff.next_delay(), Duration::from_millis(240));
        assert_eq!(backoff.next_delay(), Duration::from_millis(300));
        assert_eq!(backoff.next_delay(), Duration::from_millis(300));
    }

    #[test]
    fn reset() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(60), Duration::from_millis(300));
        assert_eq!(backoff.next_delay(), Duration::from_millis(60));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(60));
        assert_eq!(backoff.next_delay(), Duration::from_millis(120));
    }
}
