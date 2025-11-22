use std::time::{SystemTime, UNIX_EPOCH};

/// High-precision timestamp in nanoseconds since UNIX epoch
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Get current timestamp with nanosecond precision
    #[inline(always)]
    pub fn now() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        Self(now.as_nanos() as u64)
    }

    /// Create timestamp from nanoseconds
    #[inline(always)]
    pub fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Get nanoseconds value
    #[inline(always)]
    pub fn as_nanos(&self) -> u64 {
        self.0
    }

    /// Calculate latency in nanoseconds from this timestamp to now
    #[inline(always)]
    pub fn latency_ns(&self) -> u64 {
        Self::now().0.saturating_sub(self.0)
    }

    /// Calculate latency in microseconds from this timestamp to now
    #[inline(always)]
    pub fn latency_us(&self) -> u64 {
        self.latency_ns() / 1_000
    }

    /// Calculate latency in milliseconds from this timestamp to now
    #[inline(always)]
    pub fn latency_ms(&self) -> u64 {
        self.latency_ns() / 1_000_000
    }
}

impl From<u64> for Timestamp {
    fn from(nanos: u64) -> Self {
        Self(nanos)
    }
}

impl From<Timestamp> for u64 {
    fn from(ts: Timestamp) -> Self {
        ts.0
    }
}

/// Latency tracker for measuring system performance
pub struct LatencyTracker {
    name: String,
    start: Timestamp,
}

impl LatencyTracker {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: Timestamp::now(),
        }
    }

    pub fn elapsed_ns(&self) -> u64 {
        self.start.latency_ns()
    }

    pub fn elapsed_us(&self) -> u64 {
        self.start.latency_us()
    }

    pub fn log_elapsed(&self) {
        tracing::debug!(
            target: "latency",
            name = %self.name,
            elapsed_ns = self.elapsed_ns(),
            elapsed_us = self.elapsed_us(),
            "Latency measurement"
        );
    }
}

impl Drop for LatencyTracker {
    fn drop(&mut self) {
        self.log_elapsed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_timestamp_ordering() {
        let t1 = Timestamp::now();
        thread::sleep(Duration::from_nanos(100));
        let t2 = Timestamp::now();
        assert!(t2 > t1);
    }

    #[test]
    fn test_latency_measurement() {
        let ts = Timestamp::now();
        thread::sleep(Duration::from_micros(100));
        let latency_us = ts.latency_us();
        assert!(latency_us >= 100);
        assert!(latency_us < 1_000); // Should be less than 1ms
    }
}
