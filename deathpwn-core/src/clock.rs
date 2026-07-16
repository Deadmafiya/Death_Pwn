/// Wall-clock source in milliseconds since the Unix epoch. Injected everywhere
/// timing matters (failover latency, artifact dir names) so tests never touch
/// the real clock.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Real clock backed by `SystemTime`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(any(test, feature = "test-support"))]
use std::collections::VecDeque;
#[cfg(any(test, feature = "test-support"))]
use std::sync::Mutex;

/// Test clock: returns scripted timestamps in order, then repeats the last one.
/// Shared across tasks via the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeClock {
    times: Mutex<VecDeque<u64>>,
    last: Mutex<u64>,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeClock {
    pub fn new(times: Vec<u64>) -> Self {
        FakeClock {
            times: Mutex::new(times.into_iter().collect()),
            last: Mutex::new(0),
        }
    }

    pub fn fixed(t: u64) -> Self {
        FakeClock::new(vec![t])
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        let mut q = self.times.lock().expect("FakeClock times mutex poisoned");
        match q.pop_front() {
            Some(v) => {
                *self.last.lock().expect("FakeClock last mutex poisoned") = v;
                v
            }
            None => *self.last.lock().expect("FakeClock last mutex poisoned"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_replays_scripted_times_then_holds_last() {
        let clock = FakeClock::new(vec![1_000, 1_200, 1_500]);
        assert_eq!(clock.now_ms(), 1_000);
        assert_eq!(clock.now_ms(), 1_200);
        assert_eq!(clock.now_ms(), 1_500);
        // Exhausted script → repeats the final value (stable for latency math).
        assert_eq!(clock.now_ms(), 1_500);
    }

    #[test]
    fn fake_clock_fixed_always_returns_same_value() {
        let clock = FakeClock::fixed(42);
        assert_eq!(clock.now_ms(), 42);
        assert_eq!(clock.now_ms(), 42);
    }

    #[test]
    fn system_clock_returns_plausible_epoch_millis() {
        let clock = SystemClock;
        // Any real run is well after 2020-01-01T00:00:00Z (1_577_836_800_000 ms).
        assert!(clock.now_ms() > 1_577_836_800_000);
    }
}
