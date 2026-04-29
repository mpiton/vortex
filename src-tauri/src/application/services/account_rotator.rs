//! `AccountRotator` — quota-aware account rotation.
//!
//! PRD §6.4 ("Rotation si quota atteint") — when a hoster signals
//! quota exhaustion (HTTP 429, `traffic_left` below threshold, …) the
//! rotator pulls the offending account out of the rotation for a
//! cooldown window, asks the [`AccountSelector`] for the next best
//! candidate, and emits a `DomainEvent::AccountExhausted` so the UI
//! can warn the user.
//!
//! The exhaustion state is held entirely in memory: the SQLite-backed
//! [`Account`] aggregate intentionally does not persist
//! `exhausted_until` (a fresh `Account::reconstruct` always returns
//! `exhausted_until == None`). Storing it in a process-local map means
//! a restart wipes the cooldown — that is the desired behaviour for a
//! 5-to-15 minute window. Persisting it would need a new SQLite column
//! plus a purge job, neither of which buys correctness when the
//! upstream hoster will simply re-send the same 429.
//!
//! Concurrency: the map sits behind a `std::sync::Mutex`. Every public
//! method that takes the lock surfaces a poisoned mutex as
//! `AppError::Validation` instead of folding it into `Ok(None)`, so a
//! caller can distinguish "nothing eligible" from "internal state
//! corrupted" — same contract as `AccountSelector::pick_round_robin`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::error::AppError;
use crate::application::services::AccountSelector;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountId, AccountSelectionStrategy};
use crate::domain::ports::driven::AccountRepository;
use crate::domain::ports::driven::clock::Clock;
use crate::domain::ports::driven::event_bus::EventBus;

/// Outcome of [`AccountRotator::next_account`].
///
/// Distinguishes the three states callers must react to differently:
/// pick a credential, fall back to the free / unauthenticated path, or
/// stall the download in `Waiting` until the cooldown expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAccountOutcome {
    /// The rotator picked a non-exhausted account.
    Picked(Account),
    /// The service has zero registered accounts (or all are
    /// disabled / expired). Callers should fall back to the free
    /// hoster path or surface a `NoAccountConfigured` UI hint.
    NoneAvailable,
    /// At least one account exists but every eligible candidate is
    /// currently exhausted. Callers should mark the download
    /// `Waiting` until `next_eligible_at_ms` (Unix epoch ms — the
    /// earliest cooldown deadline among the exhausted set) so the
    /// scheduler can retry without busy-looping.
    AllExhausted { next_eligible_at_ms: u64 },
}

impl NextAccountOutcome {
    /// Standard human-readable message a caller can attach to the
    /// `Download.error` field when the outcome is not `Picked`. Returns
    /// `None` for `Picked` (the caller has a credential, no error to
    /// report). The wording is frozen by PRD §6.4 so the UI / log /
    /// notification copy stays consistent across hosters.
    pub fn error_message(&self, service_name: &str) -> Option<String> {
        match self {
            Self::Picked(_) => None,
            Self::NoneAvailable => Some(format!("No account available for {service_name}")),
            Self::AllExhausted { .. } => Some(format!("All accounts exhausted for {service_name}")),
        }
    }
}

/// Central account rotation service. Wraps an [`AccountSelector`] and
/// adds an in-memory cooldown map keyed by [`AccountId`].
pub struct AccountRotator {
    selector: Arc<AccountSelector>,
    repo: Arc<dyn AccountRepository>,
    event_bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    /// `account_id → cooldown deadline (Unix epoch ms)`. An entry whose
    /// deadline is `<= now_ms` is considered expired and pruned on the
    /// next read. This avoids needing a background sweeper.
    exhausted: Mutex<HashMap<AccountId, u64>>,
}

impl AccountRotator {
    pub fn new(
        selector: Arc<AccountSelector>,
        repo: Arc<dyn AccountRepository>,
        event_bus: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            selector,
            repo,
            event_bus,
            clock,
            exhausted: Mutex::new(HashMap::new()),
        })
    }

    /// Pick the next eligible account for `service_name`, skipping any
    /// account whose cooldown is still active.
    ///
    /// Distinguishes "zero accounts" (`NoneAvailable`) from "all
    /// exhausted" (`AllExhausted { next_eligible_at_ms }`) so the
    /// caller can decide whether to fall back to a free path or stall
    /// the download in `Waiting` until the cooldown expires.
    pub fn next_account(
        &self,
        service_name: &str,
        strategy: AccountSelectionStrategy,
    ) -> Result<NextAccountOutcome, AppError> {
        let now_ms = self.now_ms();
        let mut exhausted_ids = self.snapshot_exhausted(now_ms)?;
        // Linearise with concurrent `mark_exhausted` / `clear_exhausted`:
        // - On every pick, re-check the chosen id under the lock and
        //   retry with that id added to the exclude list when a
        //   parallel `mark_exhausted` landed in the gap.
        // - When the selector exhausts options, re-snapshot the
        //   cooldown map and retry once more if any id from the full
        //   current exclude list (initial snapshot ∪ race-pushed ids)
        //   has since been cleared via `clear_exhausted` or
        //   `record_traffic_refresh`. Otherwise we'd return
        //   `AllExhausted` while a live account is in fact selectable.
        loop {
            let picked = self.selector.select_best_excluding_quiet(
                service_name,
                strategy,
                &exhausted_ids,
            )?;
            if let Some(account) = picked {
                let still_available = {
                    let guard = self.lock_exhausted()?;
                    guard
                        .get(account.id())
                        .is_none_or(|deadline| now_ms >= *deadline)
                };
                if still_available {
                    // Emit AccountSelected only on the committed pick, not
                    // on probes that lose the race to a parallel
                    // `mark_exhausted`. Otherwise UI/telemetry would see
                    // "selected" for an account never returned to the caller.
                    self.event_bus.publish(DomainEvent::AccountSelected {
                        id: account.id().clone(),
                        service_name: service_name.to_string(),
                        strategy: strategy.to_string(),
                    });
                    return Ok(NextAccountOutcome::Picked(account));
                }
                exhausted_ids.push(account.id().clone());
                continue;
            }
            // No pick under the current exclude list. Re-snapshot the
            // cooldown map and retry if any id we were previously
            // excluding (including race-pushed ones) has been cleared.
            let fresh = self.snapshot_exhausted(now_ms)?;
            let any_cleared = exhausted_ids.iter().any(|id| !fresh.contains(id));
            if !any_cleared {
                break;
            }
            exhausted_ids = fresh;
        }
        // No pick after stable re-snapshot. Decide between NoneAvailable
        // and AllExhausted by looking at the repo directly: if there's
        // at least one enabled, non-expired account for this service,
        // the rotation is the blocker, not the absence of credentials.
        let candidates = self.repo.list_by_service(service_name)?;
        let live: Vec<&Account> = candidates
            .iter()
            .filter(|a| a.is_enabled() && !a.is_expired(now_ms))
            .collect();
        if live.is_empty() {
            return Ok(NextAccountOutcome::NoneAvailable);
        }
        let next_eligible_at_ms = self.earliest_deadline_for_service(&live, now_ms)?;
        Ok(NextAccountOutcome::AllExhausted {
            next_eligible_at_ms,
        })
    }

    /// Mark `account_id` as quota-exhausted for `ttl_secs` seconds.
    /// Callers pass a hoster-specific cooldown (typical range: a few
    /// hundred seconds for free plans, longer for daily caps). Emits
    /// [`DomainEvent::AccountExhausted`] carrying the committed deadline.
    ///
    /// If a cooldown entry already exists and its deadline is further
    /// in the future than the proposed one, the existing deadline
    /// wins. This prevents a short retry-driven TTL from accidentally
    /// shortening a longer daily-cap cooldown set by a previous
    /// signal.
    pub fn mark_exhausted(
        &self,
        account_id: &AccountId,
        service_name: &str,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let now_ms = self.now_ms();
        let proposed = now_ms.saturating_add(ttl_secs.saturating_mul(1_000));
        let committed = {
            let mut guard = self.lock_exhausted()?;
            let final_deadline = match guard.get(account_id) {
                Some(existing) if *existing > proposed => *existing,
                _ => proposed,
            };
            guard.insert(account_id.clone(), final_deadline);
            final_deadline
        };
        self.event_bus.publish(DomainEvent::AccountExhausted {
            id: account_id.clone(),
            service_name: service_name.to_string(),
            exhausted_until_ms: committed,
        });
        Ok(())
    }

    /// Drop any cooldown entry for `account_id` regardless of its
    /// remaining TTL. Idempotent — calling on an unknown id is a
    /// no-op.
    pub fn clear_exhausted(&self, account_id: &AccountId) -> Result<(), AppError> {
        let mut guard = self.lock_exhausted()?;
        guard.remove(account_id);
        Ok(())
    }

    /// `true` when `account_id` has an active cooldown at the current
    /// clock reading. Expired entries are NOT pruned by this call —
    /// pruning happens lazily inside `next_account` /
    /// `snapshot_exhausted`. The check is read-only by design so it can
    /// be called from log paths without surprising state changes.
    pub fn is_exhausted(&self, account_id: &AccountId) -> Result<bool, AppError> {
        let now_ms = self.now_ms();
        let guard = self.lock_exhausted()?;
        Ok(guard
            .get(account_id)
            .is_some_and(|deadline| now_ms < *deadline))
    }

    /// Hoster-agnostic quota signal. Returns `true` when an HTTP
    /// response should be treated as quota exhaustion:
    ///
    /// * `http_status == 429` (Too Many Requests — the unambiguous
    ///   quota signal)
    /// * `traffic_left.is_some()` AND below `threshold_bytes` (the
    ///   remaining quota dropped under the configured floor)
    ///
    /// Hoster-specific patterns (e.g. body string `"daily limit"`)
    /// belong in the plugin layer; the rotator stays generic.
    pub fn is_quota_signal(
        http_status: u16,
        traffic_left: Option<u64>,
        threshold_bytes: u64,
    ) -> bool {
        if http_status == 429 {
            return true;
        }
        matches!(traffic_left, Some(left) if left < threshold_bytes)
    }

    /// Reconcile a freshly observed `traffic_left` against the
    /// exhaustion map. When the upstream confirms `traffic_left` is at
    /// or above `threshold_bytes`, drop the cooldown so the next
    /// `next_account` call can pick the account again. When the
    /// observation is below the threshold OR `None` (unknown), the
    /// cooldown is left untouched — `mark_exhausted` is the canonical
    /// way to extend it.
    pub fn record_traffic_refresh(
        &self,
        account_id: &AccountId,
        traffic_left: Option<u64>,
        threshold_bytes: u64,
    ) -> Result<(), AppError> {
        let confirms_available = matches!(traffic_left, Some(left) if left >= threshold_bytes);
        if !confirms_available {
            return Ok(());
        }
        self.clear_exhausted(account_id)
    }

    fn snapshot_exhausted(&self, now_ms: u64) -> Result<Vec<AccountId>, AppError> {
        let mut guard = self.lock_exhausted()?;
        guard.retain(|_, deadline| now_ms < *deadline);
        Ok(guard.keys().cloned().collect())
    }

    fn earliest_deadline_for_service(
        &self,
        live_candidates: &[&Account],
        now_ms: u64,
    ) -> Result<u64, AppError> {
        // Restrict the deadline scan to accounts that actually belong
        // to the queried service so a parallel-service entry cannot
        // leak its cooldown into an unrelated `AllExhausted` answer.
        let guard = self.lock_exhausted()?;
        let next = live_candidates
            .iter()
            .filter_map(|acc| guard.get(acc.id()).copied())
            .filter(|deadline| now_ms < *deadline)
            .min()
            .unwrap_or(now_ms);
        Ok(next)
    }

    fn lock_exhausted(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<AccountId, u64>>, AppError> {
        self.exhausted
            .lock()
            .map_err(|_| AppError::Validation("exhausted accounts mutex poisoned".to_string()))
    }

    fn now_ms(&self) -> u64 {
        self.clock.now_unix_secs().saturating_mul(1_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::AccountSelector;
    use crate::domain::error::DomainError;
    use crate::domain::model::account::{Account, AccountType};
    use crate::domain::ports::driven::AccountRepository;
    use std::sync::Mutex as StdMutex;

    // --- Inline mocks (mirroring account_selector tests) ---

    struct InMemoryRepo {
        accounts: StdMutex<Vec<Account>>,
    }

    impl InMemoryRepo {
        fn new(accounts: Vec<Account>) -> Self {
            Self {
                accounts: StdMutex::new(accounts),
            }
        }
    }

    impl AccountRepository for InMemoryRepo {
        fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.id() == id)
                .cloned())
        }

        fn save(&self, account: &Account) -> Result<(), DomainError> {
            let mut guard = self.accounts.lock().unwrap();
            if let Some(existing) = guard.iter_mut().find(|a| a.id() == account.id()) {
                *existing = account.clone();
            } else {
                guard.push(account.clone());
            }
            Ok(())
        }

        fn list(&self) -> Result<Vec<Account>, DomainError> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.service_name() == service_name)
                .cloned()
                .collect())
        }

        fn delete(&self, id: &AccountId) -> Result<(), DomainError> {
            self.accounts.lock().unwrap().retain(|a| a.id() != id);
            Ok(())
        }
    }

    type EventSubscriber = Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>;

    struct CollectingBus {
        events: StdMutex<Vec<DomainEvent>>,
        subscribers: StdMutex<Vec<EventSubscriber>>,
    }

    impl CollectingBus {
        fn new() -> Self {
            Self {
                events: StdMutex::new(Vec::new()),
                subscribers: StdMutex::new(Vec::new()),
            }
        }

        fn events(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EventBus for CollectingBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event.clone());
            for handler in self.subscribers.lock().unwrap().iter() {
                handler(&event);
            }
        }

        fn subscribe(&self, handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {
            self.subscribers.lock().unwrap().push(handler);
        }
    }

    /// Mutable clock that tests advance manually so we never rely on
    /// `std::time::Instant` (which would couple tests to wall-clock).
    struct TestClock {
        now_secs: StdMutex<u64>,
    }

    impl TestClock {
        fn new(now_secs: u64) -> Arc<Self> {
            Arc::new(Self {
                now_secs: StdMutex::new(now_secs),
            })
        }

        fn advance_secs(&self, delta: u64) {
            let mut g = self.now_secs.lock().unwrap();
            *g = g.saturating_add(delta);
        }
    }

    impl Clock for TestClock {
        fn now_unix_secs(&self) -> u64 {
            *self.now_secs.lock().unwrap()
        }
    }

    fn account(id: &str, service: &str, traffic_left: Option<u64>, enabled: bool) -> Account {
        Account::reconstruct(
            AccountId::new(id),
            service.to_string(),
            format!("user-{id}"),
            AccountType::Premium,
            enabled,
            traffic_left,
            None,
            // Far in the future so `is_expired` never fires in these
            // tests — exhaustion logic is the focus, not expiry.
            Some(u64::MAX),
            Some(0),
            0,
        )
    }

    fn build_rotator(
        accounts: Vec<Account>,
        clock_secs: u64,
    ) -> (Arc<AccountRotator>, Arc<CollectingBus>, Arc<TestClock>) {
        let repo: Arc<dyn AccountRepository> = Arc::new(InMemoryRepo::new(accounts));
        let bus = Arc::new(CollectingBus::new());
        let clock = TestClock::new(clock_secs);
        let selector = AccountSelector::new(repo.clone(), bus.clone(), clock.clone());
        let rotator = AccountRotator::new(selector, repo, bus.clone(), clock.clone());
        (rotator, bus, clock)
    }

    // --- AC #1: 429 → rotation vers 2ème account visible ---
    #[test]
    fn test_mark_exhausted_routes_next_account_to_remaining_candidate() {
        let a = account("a", "Uploaded", Some(50_000_000_000), true);
        let b = account("b", "Uploaded", Some(40_000_000_000), true);
        let (rotator, bus, _clock) = build_rotator(vec![a, b], 1_700_000_000);

        let first = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match first {
            NextAccountOutcome::Picked(acc) => assert_eq!(acc.id().as_str(), "a"),
            other => panic!("expected Picked(a), got {other:?}"),
        }

        rotator
            .mark_exhausted(&AccountId::new("a"), "Uploaded", 600)
            .unwrap();
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());

        let second = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match second {
            NextAccountOutcome::Picked(acc) => assert_eq!(acc.id().as_str(), "b"),
            other => panic!("expected Picked(b), got {other:?}"),
        }

        let events = bus.events();
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::AccountExhausted { id, service_name, exhausted_until_ms: _ }
                if id.as_str() == "a" && service_name == "Uploaded"
        )));
    }

    // --- AC #2: tous accounts 429 → AllExhausted ---
    #[test]
    fn test_next_account_returns_all_exhausted_when_every_candidate_is_marked() {
        let a = account("a", "Uploaded", Some(50), true);
        let b = account("b", "Uploaded", Some(40), true);
        let (rotator, _bus, _clock) = build_rotator(vec![a, b], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "Uploaded", 600)
            .unwrap();
        rotator
            .mark_exhausted(&AccountId::new("b"), "Uploaded", 1200)
            .unwrap();

        let outcome = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match outcome {
            NextAccountOutcome::AllExhausted {
                next_eligible_at_ms,
            } => {
                let now_ms = 1_700_000_000_u64.saturating_mul(1_000);
                let earliest = now_ms.saturating_add(600 * 1_000);
                assert_eq!(
                    next_eligible_at_ms, earliest,
                    "must report the EARLIEST cooldown deadline"
                );
            }
            other => panic!("expected AllExhausted, got {other:?}"),
        }
    }

    #[test]
    fn test_next_account_returns_none_available_when_service_has_no_account() {
        let (rotator, _bus, _clock) = build_rotator(vec![], 1_700_000_000);
        let outcome = rotator
            .next_account("UnknownService", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        assert_eq!(outcome, NextAccountOutcome::NoneAvailable);
    }

    #[test]
    fn test_next_account_returns_none_available_when_only_disabled_accounts() {
        let a = account("a", "Uploaded", Some(50), false);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);
        let outcome = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        assert_eq!(outcome, NextAccountOutcome::NoneAvailable);
    }

    // --- AC #3: reset après refresh confirme dispo ---
    #[test]
    fn test_record_traffic_refresh_clears_cooldown_when_confirms_available() {
        let a = account("a", "Uploaded", Some(50), true);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "Uploaded", 600)
            .unwrap();
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());

        // Refresh observes plenty of traffic available → clear.
        rotator
            .record_traffic_refresh(&AccountId::new("a"), Some(50_000_000), 1_000)
            .unwrap();
        assert!(!rotator.is_exhausted(&AccountId::new("a")).unwrap());
    }

    #[test]
    fn test_record_traffic_refresh_keeps_cooldown_when_below_threshold() {
        let a = account("a", "Uploaded", Some(50), true);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "Uploaded", 600)
            .unwrap();
        // Refresh observes traffic STILL below threshold → keep
        // cooldown, do not flip-flop.
        rotator
            .record_traffic_refresh(&AccountId::new("a"), Some(500), 1_000)
            .unwrap();
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());
    }

    #[test]
    fn test_record_traffic_refresh_keeps_cooldown_when_traffic_unknown() {
        // `traffic_left == None` is the "unknown" case (e.g. hoster
        // does not expose a counter). The rotator must NOT clear the
        // cooldown on a None observation — that would silently undo
        // every `mark_exhausted` for hosters with no traffic API.
        let a = account("a", "S", None, true);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 600)
            .unwrap();
        rotator
            .record_traffic_refresh(&AccountId::new("a"), None, 1_000)
            .unwrap();
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());
    }

    #[test]
    fn test_clear_exhausted_drops_cooldown_explicitly() {
        let a = account("a", "S", Some(50), true);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);
        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 600)
            .unwrap();
        rotator.clear_exhausted(&AccountId::new("a")).unwrap();
        assert!(!rotator.is_exhausted(&AccountId::new("a")).unwrap());
    }

    #[test]
    fn test_clear_exhausted_is_noop_for_unknown_id() {
        let (rotator, _bus, _clock) = build_rotator(vec![], 1_700_000_000);
        rotator
            .clear_exhausted(&AccountId::new("ghost"))
            .expect("clearing an unknown id is a no-op, not an error");
    }

    #[test]
    fn test_cooldown_expires_after_ttl_so_account_picks_back_up() {
        let a = account("a", "S", Some(50), true);
        let (rotator, _bus, clock) = build_rotator(vec![a], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 60)
            .unwrap();
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());

        clock.advance_secs(61);
        assert!(!rotator.is_exhausted(&AccountId::new("a")).unwrap());

        let outcome = rotator
            .next_account("S", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match outcome {
            NextAccountOutcome::Picked(acc) => assert_eq!(acc.id().as_str(), "a"),
            other => panic!("expected Picked(a) after cooldown, got {other:?}"),
        }
    }

    #[test]
    fn test_is_quota_signal_detects_429_regardless_of_traffic() {
        assert!(AccountRotator::is_quota_signal(429, None, 1_000));
        assert!(AccountRotator::is_quota_signal(429, Some(u64::MAX), 1_000));
    }

    #[test]
    fn test_is_quota_signal_detects_traffic_below_threshold() {
        assert!(AccountRotator::is_quota_signal(200, Some(500), 1_000));
        assert!(AccountRotator::is_quota_signal(200, Some(0), 1_000));
    }

    #[test]
    fn test_is_quota_signal_ignores_normal_responses_above_threshold() {
        assert!(!AccountRotator::is_quota_signal(200, Some(2_000), 1_000));
        assert!(!AccountRotator::is_quota_signal(200, None, 1_000));
        assert!(!AccountRotator::is_quota_signal(404, None, 1_000));
        assert!(!AccountRotator::is_quota_signal(500, Some(50_000), 1_000));
    }

    #[test]
    fn test_is_quota_signal_threshold_is_exclusive_at_equality() {
        // Equal traffic vs threshold should NOT trip the signal —
        // matches the "below threshold" copy in PRD §6.4. This freezes
        // the rule so a future change cannot quietly invert it.
        assert!(!AccountRotator::is_quota_signal(200, Some(1_000), 1_000));
    }

    /// Integration-flavour test: simulate the full quota detection →
    /// rotation flow as the calling plugin would orchestrate it.
    /// `is_quota_signal` decides exhaustion → `mark_exhausted` →
    /// `next_account` returns the second candidate. Primary is sized
    /// with more traffic so `BestTraffic` picks it first.
    #[test]
    fn test_quota_detection_to_rotation_full_flow() {
        let a = account("primary", "Uploaded", Some(50_000_000), true);
        let b = account("backup", "Uploaded", Some(500), true);
        let (rotator, bus, _clock) = build_rotator(vec![a, b], 1_700_000_000);

        // Step 1: caller picks the primary (more traffic wins).
        let first = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        let primary = match first {
            NextAccountOutcome::Picked(acc) => acc,
            other => panic!("expected Picked(primary), got {other:?}"),
        };
        assert_eq!(primary.id().as_str(), "primary");

        // Step 2: hoster responds 429 → caller checks the heuristic.
        // Pass `Some(0)` to mimic a real "exhausted on the wire" case;
        // the 429 alone is enough but exercising the traffic branch
        // makes the assertion robust against future rule changes.
        let exhausted = AccountRotator::is_quota_signal(429, Some(0), 1_000);
        assert!(exhausted);

        // Step 3: caller marks it exhausted with a hoster-supplied TTL.
        rotator
            .mark_exhausted(primary.id(), primary.service_name(), 300)
            .unwrap();

        // Step 4: rotator now picks the backup.
        let second = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match second {
            NextAccountOutcome::Picked(acc) => assert_eq!(acc.id().as_str(), "backup"),
            other => panic!("expected Picked(backup), got {other:?}"),
        }

        let event_count = bus
            .events()
            .iter()
            .filter(|e| matches!(e, DomainEvent::AccountExhausted { .. }))
            .count();
        assert_eq!(
            event_count, 1,
            "exactly one AccountExhausted should have been emitted"
        );
    }

    #[test]
    fn test_mark_exhausted_handles_zero_ttl_gracefully() {
        // A zero TTL is degenerate but not a bug; the rotator must not
        // panic. The deadline equals `now_ms`, which `is_exhausted`
        // treats as "just elapsed" (deadline is exclusive).
        let a = account("a", "S", Some(50), true);
        let (rotator, _bus, _clock) = build_rotator(vec![a], 1_700_000_000);

        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 0)
            .unwrap();
        assert!(
            !rotator.is_exhausted(&AccountId::new("a")).unwrap(),
            "ttl=0 means the cooldown has already expired at now"
        );
    }

    #[test]
    fn test_mark_exhausted_keeps_existing_longer_deadline() {
        // A long cooldown (daily cap) followed by a short retry signal
        // must not shrink the active cooldown. The committed deadline
        // wins, and the AccountExhausted event publishes it verbatim
        // so subscribers don't see a phantom shorter window.
        let a = account("a", "S", Some(50), true);
        let (rotator, bus, clock) = build_rotator(vec![a], 1_700_000_000);
        let now_ms = 1_700_000_000_u64 * 1_000;

        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 600)
            .unwrap();
        let long_deadline = now_ms + 600 * 1_000;

        rotator
            .mark_exhausted(&AccountId::new("a"), "S", 60)
            .unwrap();

        // The shorter retry signal would expire after 60s. Advance
        // past that and confirm the cooldown is still active —
        // proving the longer (600s) deadline stuck.
        clock.advance_secs(120);
        assert!(rotator.is_exhausted(&AccountId::new("a")).unwrap());

        // Advance past the long deadline; cooldown finally clears.
        clock.advance_secs(600);
        assert!(!rotator.is_exhausted(&AccountId::new("a")).unwrap());

        let payloads: Vec<u64> = bus
            .events()
            .iter()
            .filter_map(|e| match e {
                DomainEvent::AccountExhausted {
                    exhausted_until_ms, ..
                } => Some(*exhausted_until_ms),
                _ => None,
            })
            .collect();
        assert_eq!(
            payloads,
            vec![long_deadline, long_deadline],
            "second AccountExhausted must republish the still-active longer deadline, not the shorter proposed one"
        );
    }

    /// PRD §6.4 freezes the human-facing message format. Callers that
    /// translate `AllExhausted` into a `Download.error` rely on this
    /// exact wording so the UI / notification text stays uniform across
    /// hosters.
    #[test]
    fn test_outcome_error_message_uses_prd_wording() {
        let outcome = NextAccountOutcome::AllExhausted {
            next_eligible_at_ms: 1_700_000_000_000,
        };
        assert_eq!(
            outcome.error_message("Uploaded"),
            Some("All accounts exhausted for Uploaded".to_string()),
        );

        let none = NextAccountOutcome::NoneAvailable;
        assert_eq!(
            none.error_message("Mediafire"),
            Some("No account available for Mediafire".to_string()),
        );

        let a = account("a", "S", Some(1), true);
        let picked = NextAccountOutcome::Picked(a);
        assert_eq!(
            picked.error_message("S"),
            None,
            "Picked is the success path — no error message"
        );
    }

    #[test]
    fn test_all_exhausted_deadline_uses_only_this_services_accounts() {
        let primary = account("a", "Uploaded", Some(50), true);
        let cross_service = account("b", "Mediafire", Some(50), true);
        let (rotator, _bus, _clock) = build_rotator(vec![primary, cross_service], 1_700_000_000);

        // Mark BOTH exhausted but with very different deadlines.
        rotator
            .mark_exhausted(&AccountId::new("a"), "Uploaded", 100)
            .unwrap();
        rotator
            .mark_exhausted(&AccountId::new("b"), "Mediafire", 99_999)
            .unwrap();

        let outcome = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        match outcome {
            NextAccountOutcome::AllExhausted {
                next_eligible_at_ms,
            } => {
                let now_ms = 1_700_000_000_u64 * 1_000;
                assert_eq!(
                    next_eligible_at_ms,
                    now_ms + 100 * 1_000,
                    "Mediafire's longer cooldown must NOT leak into Uploaded's deadline"
                );
            }
            other => panic!("expected AllExhausted, got {other:?}"),
        }
    }

    #[test]
    fn test_next_account_emits_account_selected_exactly_once_on_picked() {
        // Regression: rotator now drives `AccountSelected` emission
        // itself (selector probes go via `_quiet`). The contract is
        // "one Picked outcome = one AccountSelected event" — never
        // zero, never several.
        let a = account("only", "Uploaded", Some(1_000), true);
        let (rotator, bus, _clock) = build_rotator(vec![a], 1_700_000_000);

        let outcome = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        assert!(matches!(outcome, NextAccountOutcome::Picked(_)));

        let selected_count = bus
            .events()
            .iter()
            .filter(|e| matches!(e, DomainEvent::AccountSelected { .. }))
            .count();
        assert_eq!(
            selected_count, 1,
            "rotator must emit exactly one AccountSelected per Picked outcome"
        );
    }

    #[test]
    fn test_next_account_does_not_emit_account_selected_on_none() {
        // No accounts configured. Path returns NoneAvailable and
        // must not produce an AccountSelected event (it would only
        // be possible via the selector's old emission point, which
        // moved into the rotator's commit branch).
        let (rotator, bus, _clock) = build_rotator(vec![], 1_700_000_000);

        let outcome = rotator
            .next_account("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        assert!(matches!(outcome, NextAccountOutcome::NoneAvailable));

        let selected_count = bus
            .events()
            .iter()
            .filter(|e| matches!(e, DomainEvent::AccountSelected { .. }))
            .count();
        assert_eq!(selected_count, 0);
    }
}
