//! Time source. Injected so tests are deterministic (NFR-018: UTC, millisecond precision).

use std::sync::atomic::{AtomicI64, Ordering};

use time::OffsetDateTime;
use time::macros::format_description;

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// Deterministic clock for tests: starts at a fixed instant and advances 1 ms per read.
#[derive(Debug)]
pub struct StepClock {
    next_ms: AtomicI64,
}

impl StepClock {
    #[must_use]
    pub fn starting_at_unix_ms(ms: i64) -> Self {
        Self {
            next_ms: AtomicI64::new(ms),
        }
    }
}

impl Default for StepClock {
    fn default() -> Self {
        // 2026-10-24T00:00:00Z
        Self::starting_at_unix_ms(1_792_800_000_000)
    }
}

impl Clock for StepClock {
    fn now(&self) -> OffsetDateTime {
        let ms = self.next_ms.fetch_add(1, Ordering::Relaxed);
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .expect("test timestamps are in range")
    }
}

/// Test clock that only moves when told to; reads do not advance it.
#[derive(Debug)]
pub struct ManualClock {
    ms: AtomicI64,
}

impl ManualClock {
    #[must_use]
    pub fn new() -> Self {
        // 2026-10-24T00:00:00Z
        Self {
            ms: AtomicI64::new(1_792_800_000_000),
        }
    }

    pub fn advance(&self, by: std::time::Duration) {
        let ms = i64::try_from(by.as_millis()).expect("duration fits");
        self.ms.fetch_add(ms, Ordering::Relaxed);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp_nanos(
            i128::from(self.ms.load(Ordering::Relaxed)) * 1_000_000,
        )
        .expect("test timestamps are in range")
    }
}

/// `2026-10-24T09:14:03.120Z`: fixed-width RFC 3339 UTC with milliseconds.
pub fn format_ts(t: OffsetDateTime) -> Result<String, time::error::Format> {
    t.to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_fixed_width_utc_millis() {
        let c = StepClock::default();
        assert_eq!(
            format_ts(c.now()).expect("format"),
            "2026-10-24T00:00:00.000Z"
        );
        assert_eq!(
            format_ts(c.now()).expect("format"),
            "2026-10-24T00:00:00.001Z"
        );
    }
}
