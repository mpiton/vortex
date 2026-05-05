//! Wall-clock abstraction.
//!
//! Lets long-running schedulers (history retention purge, future
//! statistics rollups, etc.) read "now" through an injected port so
//! tests can drive time deterministically without sleeping or mocking
//! `SystemTime` globally.

/// Returns the current wall-clock time as Unix epoch seconds.
///
/// Implementations MUST be cheap to call and side-effect free.
/// `Send + Sync` is required so the trait object can be shared across
/// tokio tasks via `Arc`.
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch (UTC, leap-second-ignorant).
    fn now_unix_secs(&self) -> u64;

    /// Milliseconds since the Unix epoch. Default derives from
    /// `now_unix_secs` for adapters that only have second precision.
    /// Adapters with millisecond-accurate clocks should override this.
    fn now_unix_ms(&self) -> u64 {
        self.now_unix_secs().saturating_mul(1_000)
    }
}
