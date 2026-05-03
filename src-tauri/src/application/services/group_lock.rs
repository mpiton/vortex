//! Process-wide lock shared by the package groupers
//! ([`crate::application::services::PlaylistGrouper`],
//! [`crate::application::services::SplitArchiveGrouper`]) to serialise
//! find-then-save sequences.
//!
//! Without this lock, two concurrent IPC invocations for the same
//! natural key could both observe "not found" in `find_by_external_id`
//! and each insert a new `Package`, breaking the idempotent-reuse
//! guarantee. The lock window covers only the lookup + save, never the
//! downstream event publish, so the contention window stays tiny (a
//! few SQLite writes).
//!
//! A single shared mutex is intentional. The cost of mild cross-grouper
//! serialisation is negligible (groupers run only at Link-Grabber
//! commit time, far from any hot path), and a shared mutex makes
//! reasoning about the SQLite UNIQUE-index contract trivial: at most
//! one writer per process competes for any given external_id at a
//! time.

use std::sync::{Mutex, MutexGuard, OnceLock};

fn lock() -> &'static Mutex<()> {
    static GROUP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    GROUP_LOCK.get_or_init(|| Mutex::new(()))
}

/// Acquire the shared grouper lock, recovering from a poisoned mutex
/// (a previous panic while holding the guard) instead of panicking
/// again. Domain state lives in SQLite, not in the guard, so the next
/// caller can safely proceed.
pub(crate) fn acquire_grouper_lock() -> MutexGuard<'static, ()> {
    match lock().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}
