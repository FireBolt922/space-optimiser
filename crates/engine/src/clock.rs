//! Plan freshness (phase 8 item 2). A plan expires after 15 minutes, or when
//! the computer slept after it was built: files can change while the lid is
//! closed, and a plan that looked current can be hours old.
//!
//! Sleep is detected by reading two monotonic clocks, one that stops while
//! the computer sleeps and one that does not. If the second ran ahead of the
//! first, the computer slept.

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a built plan may wait for its confirm and execute.
pub const PLAN_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// Clock drift allowed between the two readings before it counts as sleep.
const SLEEP_TOLERANCE: Duration = Duration::from_secs(2);

/// Two monotonic readings taken together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp {
    /// Stops while the computer sleeps.
    pub awake: Duration,
    /// Keeps counting while the computer sleeps.
    pub total: Duration,
}

impl Stamp {
    /// True while a plan stamped `self` may still run at `now`.
    pub fn is_fresh_at(self, now: Stamp) -> bool {
        let total = now.total.saturating_sub(self.total);
        let awake = now.awake.saturating_sub(self.awake);
        total <= PLAN_LIFETIME && total.saturating_sub(awake) <= SLEEP_TOLERANCE
    }

    /// Whole seconds left before a plan stamped `self` expires.
    pub fn secs_left_at(self, now: Stamp) -> u64 {
        PLAN_LIFETIME
            .saturating_sub(now.total.saturating_sub(self.total))
            .as_secs()
    }
}

pub trait Clock: Send + Sync {
    fn stamp(&self) -> Stamp;
}

/// The operating system's clocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn stamp(&self) -> Stamp {
        sys::stamp()
    }
}

/// A clock a test moves by hand.
#[doc(hidden)]
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Arc<Mutex<Stamp>>);

impl ManualClock {
    /// Time passes with the computer awake.
    pub fn advance(&self, by: Duration) {
        let mut stamp = self.0.lock().unwrap();
        stamp.awake += by;
        stamp.total += by;
    }

    /// The computer sleeps for `by`.
    pub fn sleep(&self, by: Duration) {
        self.0.lock().unwrap().total += by;
    }
}

impl Clock for ManualClock {
    fn stamp(&self) -> Stamp {
        *self.0.lock().unwrap()
    }
}

#[cfg(unix)]
mod sys {
    use super::Stamp;
    use std::time::Duration;

    // macOS: UPTIME_RAW stops during sleep and MONOTONIC does not.
    // Linux: MONOTONIC stops during suspend and BOOTTIME does not.
    #[cfg(target_os = "macos")]
    const AWAKE: libc::clockid_t = libc::CLOCK_UPTIME_RAW;
    #[cfg(target_os = "macos")]
    const TOTAL: libc::clockid_t = libc::CLOCK_MONOTONIC;
    #[cfg(target_os = "linux")]
    const AWAKE: libc::clockid_t = libc::CLOCK_MONOTONIC;
    #[cfg(target_os = "linux")]
    const TOTAL: libc::clockid_t = libc::CLOCK_BOOTTIME;

    pub fn stamp() -> Stamp {
        Stamp {
            awake: read(AWAKE),
            total: read(TOTAL),
        }
    }

    #[allow(clippy::unnecessary_cast)] // `timespec` field widths differ between targets
    fn read(clock: libc::clockid_t) -> Duration {
        // SAFETY: an all-zero timespec is a valid out-parameter.
        let mut now: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: `now` is writable and `clock` is a valid clock id.
        if unsafe { libc::clock_gettime(clock, &mut now) } != 0 {
            return Duration::ZERO;
        }
        Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
    }
}

/// Windows cannot execute plans until M7 (phase 9), which adds the
/// unbiased-interrupt-time reading. Until then only the lifetime applies.
#[cfg(windows)]
mod sys {
    use super::Stamp;
    use std::sync::OnceLock;
    use std::time::Instant;

    pub fn stamp() -> Stamp {
        static START: OnceLock<Instant> = OnceLock::new();
        let elapsed = START.get_or_init(Instant::now).elapsed();
        Stamp {
            awake: elapsed,
            total: elapsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn a_plan_is_fresh_for_fifteen_awake_minutes() {
        let clock = ManualClock::default();
        let built = clock.stamp();
        clock.advance(15 * MIN);
        assert!(built.is_fresh_at(clock.stamp()));
        clock.advance(Duration::from_secs(1));
        assert!(!built.is_fresh_at(clock.stamp()));
    }

    #[test]
    fn any_sleep_expires_a_plan() {
        let clock = ManualClock::default();
        let built = clock.stamp();
        clock.sleep(Duration::from_secs(3));
        assert!(!built.is_fresh_at(clock.stamp()));
    }

    #[test]
    fn the_real_clocks_read_without_sleeping() {
        let clock = SystemClock;
        let first = clock.stamp();
        assert!(first.is_fresh_at(clock.stamp()));
        assert_eq!(first.secs_left_at(first), 15 * 60);
    }
}
