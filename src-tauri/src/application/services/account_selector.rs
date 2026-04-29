//! `AccountSelector` — auto-pick the best account per service.
//!
//! PRD §6.4 — when several accounts exist for the same hoster / debrid
//! service, the engine asks the selector for the one to use *now*. The
//! selector applies the strategy currently set in `AppConfig`:
//!
//! - `BestTraffic` (default): rank candidates by *enabled* → *not expired*
//!   → most `traffic_left` (unlimited > finite) → most recent
//!   `last_validated`.
//! - `RoundRobin`: alternate over enabled, non-expired candidates ordered
//!   by id. Each `select_best(service)` advances a per-service cursor so
//!   load is spread across accounts even when all of them have the same
//!   traffic profile.
//! - `Manual`: today an alias of `BestTraffic`. The pinning UI is a
//!   future iteration; the alias keeps the config schema forward-
//!   compatible and is exercised by tests so a regression cannot quietly
//!   change the fallback.
//!
//! Reads always go straight to `AccountRepository::list_by_service`; the
//! selector intentionally caches nothing. An earlier revision kept a
//! per-service candidate cache invalidated by domain events, but the
//! production `TokioEventBus` dispatches subscribers on a spawned task
//! (`broadcast::recv` + `tokio::spawn`), so a `select_best` call landing
//! between `bus.publish(AccountUpdated)` and the subscriber firing can
//! observe stale rows. SQLite reads are cheap for the row counts the
//! selector sees in practice (≤ a few dozen accounts per service), so the
//! cache traded correctness for negligible savings.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountSelectionStrategy};
use crate::domain::ports::driven::AccountRepository;
use crate::domain::ports::driven::clock::Clock;
use crate::domain::ports::driven::event_bus::EventBus;

/// Auto-selects the best `Account` for a hoster / debrid service.
///
/// `select_best` is the single entry point. It MUST be called by every
/// flow that wants to use credentials (resolve_links, plugin
/// `download_to_file` adapters, debrid resolvers) so the selection
/// strategy is honoured uniformly.
pub struct AccountSelector {
    repo: Arc<dyn AccountRepository>,
    event_bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    /// Per-service round-robin cursor. Used only for the `RoundRobin`
    /// strategy; everything else is stateless.
    rr_cursor: Mutex<HashMap<String, usize>>,
}

impl AccountSelector {
    pub fn new(
        repo: Arc<dyn AccountRepository>,
        event_bus: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            repo,
            event_bus,
            clock,
            rr_cursor: Mutex::new(HashMap::new()),
        })
    }

    /// Pick the best candidate for `service_name` according to the
    /// requested `strategy`. Returns `None` (and emits
    /// `DomainEvent::NoAccountAvailable`) when no enabled, non-expired
    /// account is available for this service.
    pub fn select_best(
        &self,
        service_name: &str,
        strategy: AccountSelectionStrategy,
    ) -> Result<Option<Account>, AppError> {
        let candidates = self.repo.list_by_service(service_name)?;
        let now_ms = self.now_ms();
        let eligible: Vec<&Account> = candidates
            .iter()
            .filter(|a| a.is_enabled() && !a.is_expired(now_ms))
            .collect();
        if eligible.is_empty() {
            self.event_bus.publish(DomainEvent::NoAccountAvailable {
                service_name: service_name.to_string(),
            });
            return Ok(None);
        }
        let chosen = match strategy {
            AccountSelectionStrategy::BestTraffic | AccountSelectionStrategy::Manual => {
                pick_best_traffic(&eligible)
            }
            AccountSelectionStrategy::RoundRobin => self.pick_round_robin(service_name, &eligible),
        };
        let account = chosen.cloned();
        if let Some(ref acc) = account {
            self.event_bus.publish(DomainEvent::AccountSelected {
                id: acc.id().clone(),
                service_name: service_name.to_string(),
                strategy: strategy.to_string(),
            });
        }
        Ok(account)
    }

    fn pick_round_robin<'a>(&self, key: &str, eligible: &[&'a Account]) -> Option<&'a Account> {
        if eligible.is_empty() {
            return None;
        }
        let mut sorted = eligible.to_vec();
        sorted.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));
        let mut guard = self.rr_cursor.lock().ok()?;
        let cursor = guard.entry(key.to_string()).or_insert(0);
        let pick = sorted[*cursor % sorted.len()];
        *cursor = cursor.wrapping_add(1);
        Some(pick)
    }

    fn now_ms(&self) -> u64 {
        self.clock.now_unix_secs().saturating_mul(1_000)
    }
}

/// Rank rule for `BestTraffic`:
/// 1. Unlimited traffic (`traffic_left == None`) wins over any finite value.
/// 2. Among finite-traffic accounts, more `traffic_left` wins.
/// 3. Tiebreaker: most recent `last_validated` (None ranks last).
/// 4. Final tiebreaker: id ascending so the choice is deterministic.
fn pick_best_traffic<'a>(eligible: &[&'a Account]) -> Option<&'a Account> {
    eligible.iter().copied().max_by(|a, b| {
        let traffic = traffic_rank(a).cmp(&traffic_rank(b));
        if traffic != std::cmp::Ordering::Equal {
            return traffic;
        }
        let validated = a.last_validated().cmp(&b.last_validated());
        if validated != std::cmp::Ordering::Equal {
            return validated;
        }
        // Reverse so the smaller id wins (max_by returns the greatest).
        b.id().as_str().cmp(a.id().as_str())
    })
}

fn traffic_rank(a: &Account) -> TrafficRank {
    match a.traffic_left() {
        None => TrafficRank::Unlimited,
        Some(bytes) => TrafficRank::Finite(bytes),
    }
}

/// Total ordering for `traffic_left`. `Unlimited` ranks above any
/// `Finite(_)` regardless of size — an unlimited premium plan is always
/// preferable to a quota-bound one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TrafficRank {
    Finite(u64),
    Unlimited,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::account::{AccountId, AccountType};
    use std::sync::Mutex as StdMutex;

    // --- Inline mocks ---

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

    struct FixedClock(u64);

    impl Clock for FixedClock {
        fn now_unix_secs(&self) -> u64 {
            self.0
        }
    }

    fn account(
        id: &str,
        service: &str,
        traffic_left: Option<u64>,
        valid_until_ms: Option<u64>,
        last_validated_ms: Option<u64>,
        enabled: bool,
    ) -> Account {
        Account::reconstruct(
            AccountId::new(id),
            service.to_string(),
            format!("user-{id}"),
            AccountType::Premium,
            enabled,
            traffic_left,
            None,
            valid_until_ms,
            last_validated_ms,
            0,
        )
    }

    fn build_selector(
        accounts: Vec<Account>,
        now_secs: u64,
    ) -> (Arc<AccountSelector>, Arc<CollectingBus>) {
        let repo = Arc::new(InMemoryRepo::new(accounts));
        let bus = Arc::new(CollectingBus::new());
        let clock = Arc::new(FixedClock(now_secs));
        let selector = AccountSelector::new(repo, bus.clone(), clock);
        (selector, bus)
    }

    // --- Acceptance criterion 1: 1 expired, 1 low traffic, 1 full → full traffic wins ---
    #[test]
    fn test_select_best_returns_account_with_most_traffic_left() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let expired = account(
            "a-expired",
            "Uploaded",
            Some(50_000_000_000),
            Some(now_ms - 1),
            Some(now_ms - 60_000),
            true,
        );
        let low = account(
            "b-low",
            "Uploaded",
            Some(1_000_000_000),
            Some(now_ms + 86_400_000),
            Some(now_ms - 60_000),
            true,
        );
        let full = account(
            "c-full",
            "Uploaded",
            Some(50_000_000_000),
            Some(now_ms + 86_400_000),
            Some(now_ms - 60_000),
            true,
        );

        let (selector, bus) = build_selector(vec![expired, low, full], now_secs);

        let chosen = selector
            .select_best("Uploaded", AccountSelectionStrategy::BestTraffic)
            .expect("select ok")
            .expect("an account is eligible");
        assert_eq!(chosen.id().as_str(), "c-full");

        let events = bus.events();
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::AccountSelected { id, service_name, strategy }
                if id.as_str() == "c-full" && service_name == "Uploaded" && strategy == "best_traffic"
        )));
    }

    // --- Acceptance criterion 2: all expired → None + NoAccountAvailable ---
    #[test]
    fn test_select_best_returns_none_when_all_expired_and_emits_event() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let a = account("a", "Uploaded", Some(100), Some(now_ms - 10), None, true);
        let b = account("b", "Uploaded", None, Some(now_ms - 5), None, true);

        let (selector, bus) = build_selector(vec![a, b], now_secs);

        let chosen = selector
            .select_best("Uploaded", AccountSelectionStrategy::BestTraffic)
            .expect("select ok");
        assert!(chosen.is_none());

        let events = bus.events();
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::NoAccountAvailable { service_name } if service_name == "Uploaded"
        )));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountSelected { .. })),
            "must NOT emit AccountSelected when nothing was selected"
        );
    }

    // --- Acceptance criterion 3: comparative ranking table ---
    //
    // Verifies the documented rank precedence for `BestTraffic`:
    // unlimited > finite, then most-traffic, then most-recently-validated,
    // then smallest id. Each row pins one rule.
    #[test]
    fn test_select_best_unlimited_traffic_beats_any_finite() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let huge_finite = account(
            "huge",
            "S",
            Some(u64::MAX),
            Some(now_ms + 1),
            Some(now_ms),
            true,
        );
        let unlimited = account("inf", "S", None, Some(now_ms + 1), Some(now_ms), true);

        let (selector, _bus) = build_selector(vec![huge_finite, unlimited], now_secs);

        let chosen = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(chosen.id().as_str(), "inf");
    }

    #[test]
    fn test_select_best_uses_last_validated_to_break_traffic_tie() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let stale = account(
            "stale",
            "S",
            Some(10_000),
            Some(now_ms + 1),
            Some(now_ms - 60_000),
            true,
        );
        let fresh = account(
            "fresh",
            "S",
            Some(10_000),
            Some(now_ms + 1),
            Some(now_ms - 100),
            true,
        );

        let (selector, _bus) = build_selector(vec![stale, fresh], now_secs);

        let chosen = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(chosen.id().as_str(), "fresh");
    }

    #[test]
    fn test_select_best_breaks_complete_tie_with_smallest_id() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let a = account("aaa", "S", Some(5), Some(now_ms + 1), None, true);
        let b = account("bbb", "S", Some(5), Some(now_ms + 1), None, true);

        let (selector, _bus) = build_selector(vec![a, b], now_secs);

        let chosen = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(
            chosen.id().as_str(),
            "aaa",
            "deterministic id tiebreaker (smallest id wins)"
        );
    }

    #[test]
    fn test_select_best_skips_disabled_accounts() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let disabled_full = account(
            "disabled",
            "S",
            Some(u64::MAX),
            Some(now_ms + 1),
            None,
            false,
        );
        let enabled_low = account("enabled", "S", Some(1), Some(now_ms + 1), None, true);

        let (selector, _bus) = build_selector(vec![disabled_full, enabled_low], now_secs);

        let chosen = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(chosen.id().as_str(), "enabled");
    }

    // --- Acceptance criterion 4: RoundRobin alternance ---
    #[test]
    fn test_round_robin_alternates_across_eligible_accounts() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let a = account("acc-1", "S", Some(100), Some(now_ms + 1), None, true);
        let b = account("acc-2", "S", Some(200), Some(now_ms + 1), None, true);
        let c = account("acc-3", "S", Some(300), Some(now_ms + 1), None, true);

        let (selector, _bus) = build_selector(vec![a, b, c], now_secs);

        let mut picked = Vec::new();
        for _ in 0..6 {
            let chosen = selector
                .select_best("S", AccountSelectionStrategy::RoundRobin)
                .unwrap()
                .unwrap();
            picked.push(chosen.id().as_str().to_string());
        }
        assert_eq!(
            picked,
            vec!["acc-1", "acc-2", "acc-3", "acc-1", "acc-2", "acc-3"],
            "round-robin must rotate in id order and wrap around"
        );
    }

    #[test]
    fn test_round_robin_emits_account_selected_with_strategy_name() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let a = account("acc-1", "S", Some(100), Some(now_ms + 1), None, true);

        let (selector, bus) = build_selector(vec![a], now_secs);

        selector
            .select_best("S", AccountSelectionStrategy::RoundRobin)
            .unwrap()
            .unwrap();

        let events = bus.events();
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::AccountSelected { strategy, .. } if strategy == "round_robin"
        )));
    }

    #[test]
    fn test_manual_strategy_falls_back_to_best_traffic_today() {
        // Manual pinning is a future iteration; until then it must NOT
        // crash and must produce a deterministic pick (currently identical
        // to BestTraffic). Exercising it here freezes the behaviour so a
        // future change cannot quietly drop the fallback.
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let low = account("low", "S", Some(1), Some(now_ms + 1), None, true);
        let high = account("high", "S", Some(999), Some(now_ms + 1), None, true);

        let (selector, _bus) = build_selector(vec![low, high], now_secs);

        let chosen = selector
            .select_best("S", AccountSelectionStrategy::Manual)
            .unwrap()
            .unwrap();
        assert_eq!(chosen.id().as_str(), "high");
    }

    // Repo-fresh contract: `select_best` reads `list_by_service` on every
    // call, so any mutation visible in the repo surfaces on the next pick
    // without needing an event-bus round trip. This is the regression
    // pinning Codex's "synchronous-with-mutation" finding — the previous
    // event-driven cache could stay stale between `bus.publish(...)` and
    // the spawned subscriber firing under `TokioEventBus`.
    #[test]
    fn test_select_best_always_reflects_current_repo_state() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let initial = account("a", "S", Some(10), Some(now_ms + 1), None, true);

        let repo = Arc::new(InMemoryRepo::new(vec![initial]));
        let bus = Arc::new(CollectingBus::new());
        let clock = Arc::new(FixedClock(now_secs));
        let selector = AccountSelector::new(repo.clone(), bus, clock);

        let first = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(first.traffic_left(), Some(10));

        // Repo mutates with NO event published — no subscriber to notify.
        // The next call must still see the new value because the selector
        // does not cache.
        let mutated = account("a", "S", Some(9_000_000), Some(now_ms + 1), None, true);
        repo.save(&mutated).unwrap();

        let after = selector
            .select_best("S", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        assert_eq!(
            after.traffic_left(),
            Some(9_000_000),
            "selector must always read live repo state, not a snapshot"
        );
    }

    /// `service_name` lookup is whatever the repo does — `list_by_service`
    /// is case-sensitive on the SQLite `service_name` column, so a
    /// case-mismatched caller surfaces the same `None` it would on a
    /// fresh repo.
    #[test]
    fn test_select_best_is_case_sensitive_on_service_name() {
        let now_ms = 2_000_000_000_000;
        let now_secs = now_ms / 1_000;
        let a = account("a", "Uploaded", Some(10), Some(now_ms + 1), None, true);

        let repo = Arc::new(InMemoryRepo::new(vec![a]));
        let bus = Arc::new(CollectingBus::new());
        let clock = Arc::new(FixedClock(now_secs));
        let selector = AccountSelector::new(repo, bus, clock);

        let r1 = selector
            .select_best("Uploaded", AccountSelectionStrategy::BestTraffic)
            .unwrap()
            .unwrap();
        let r2 = selector
            .select_best("UPLOADED", AccountSelectionStrategy::BestTraffic)
            .unwrap();
        assert_eq!(r1.id().as_str(), "a");
        assert!(r2.is_none(), "case-mismatched service name has no rows");
    }
}
