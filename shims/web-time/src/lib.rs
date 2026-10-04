//! web-time's API without JavaScript (see `Cargo.toml`): natively std's
//! time as it is; on wasm32, a `SystemTime` read from the clock the plugin
//! sets ([`set_clock`]), Kalem's `clock` interface in a component.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::Mutex;

    pub use std::time::Duration;

    static CLOCK: Mutex<Option<fn() -> i64>> = Mutex::new(None);

    /// Sets the clock: milliseconds since January 1, 1970, UTC.
    pub fn set_clock(f: fn() -> i64) {
        if let Ok(mut c) = CLOCK.lock() {
            *c = Some(f);
        }
    }

    fn now_ms() -> i64 {
        CLOCK.lock().ok().and_then(|c| *c).map_or(0, |f| f())
    }

    /// A moment, as time since the Unix epoch (negative before it).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct SystemTime(i128);

    /// The Unix epoch.
    pub const UNIX_EPOCH: SystemTime = SystemTime(0);

    /// `earlier` was later: by how much.
    #[derive(Debug, Clone)]
    pub struct SystemTimeError(Duration);

    impl SystemTimeError {
        pub fn duration(&self) -> Duration {
            self.0
        }
    }

    impl std::fmt::Display for SystemTimeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "second time provided was later than self")
        }
    }

    impl std::error::Error for SystemTimeError {}

    fn nanos(d: Duration) -> i128 {
        d.as_nanos() as i128
    }

    fn duration(n: i128) -> Duration {
        Duration::new((n / 1_000_000_000) as u64, (n % 1_000_000_000) as u32)
    }

    impl SystemTime {
        pub const UNIX_EPOCH: SystemTime = UNIX_EPOCH;

        pub fn now() -> SystemTime {
            SystemTime(now_ms() as i128 * 1_000_000)
        }

        pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
            let d = self.0 - earlier.0;
            if d >= 0 {
                Ok(duration(d))
            } else {
                Err(SystemTimeError(duration(-d)))
            }
        }

        pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
            SystemTime::now().duration_since(*self)
        }

        pub fn checked_add(&self, d: Duration) -> Option<SystemTime> {
            self.0.checked_add(nanos(d)).map(SystemTime)
        }

        pub fn checked_sub(&self, d: Duration) -> Option<SystemTime> {
            self.0.checked_sub(nanos(d)).map(SystemTime)
        }
    }

    impl std::ops::Add<Duration> for SystemTime {
        type Output = SystemTime;
        fn add(self, d: Duration) -> SystemTime {
            SystemTime(self.0 + nanos(d))
        }
    }

    impl std::ops::Sub<Duration> for SystemTime {
        type Output = SystemTime;
        fn sub(self, d: Duration) -> SystemTime {
            SystemTime(self.0 - nanos(d))
        }
    }

    /// A moment for measuring elapsed time, from the same clock.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Instant(i128);

    impl Instant {
        pub fn now() -> Instant {
            Instant(SystemTime::now().0)
        }

        pub fn duration_since(&self, earlier: Instant) -> Duration {
            duration((self.0 - earlier.0).max(0))
        }

        pub fn elapsed(&self) -> Duration {
            Instant::now().duration_since(*self)
        }
    }
}
