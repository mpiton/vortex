# Task 03: Domain Models

## Context
Le domaine est le coeur de l'architecture hexagonale. Il contient les entites pures, la state machine des telechargements, les domain events et les domain errors. ZERO dependance externe — uniquement `std`. C'est la couche la plus testee (90%+ coverage).

## Scope
- Entites : Download, Segment, Package, Account, Plugin, CaptchaChallenge
- Value objects : DownloadId, Url, Priority, Speed, FileSize, DownloadFilter, SortField
- Enums : DownloadState, SegmentState, PluginCategory, AccountType
- State machine : transitions d'etat Download avec validation
- Domain events : DownloadStarted, DownloadCompleted, DownloadPaused, PluginLoaded...
- Domain errors : DomainError enum exhaustif

## Implementation Details

### Files to Create/Modify
- `src-tauri/src/domain/mod.rs` — Re-exports
- `src-tauri/src/domain/model/mod.rs`
- `src-tauri/src/domain/model/download.rs` — Download entity + DownloadState + state machine
- `src-tauri/src/domain/model/segment.rs` — Segment entity + SegmentState
- `src-tauri/src/domain/model/queue.rs` — Priority, QueuePosition
- `src-tauri/src/domain/model/package.rs` — Package (groupe logique)
- `src-tauri/src/domain/model/account.rs` — Account, Credential, AccountType
- `src-tauri/src/domain/model/plugin.rs` — PluginManifest, PluginInfo, PluginCategory
- `src-tauri/src/domain/model/captcha.rs` — CaptchaChallenge, CaptchaType
- `src-tauri/src/domain/event.rs` — DomainEvent enum
- `src-tauri/src/domain/error.rs` — DomainError enum

### Key Functionality

**State machine Download (PRD §6.1.2) :**
```
Queued → Downloading → Completed
           ↕              
         Paused       
           ↓
     Downloading → Waiting → Downloading
           ↓
         Retry → Downloading (si retry_count < max)
           ↓
         Error (si circuit breaker declenche)
           ↓
    Downloading → Checking → Completed
    Downloading → Extracting → Completed
```

Chaque methode de transition (`pause()`, `resume()`, `retry()`, `complete()`) retourne `Result<DomainEvent, DomainError>`.

**Circuit breaker :** Apres `max_retries` echecs consecutifs, `retry()` retourne `Err(DomainError::MaxRetriesExceeded)` et passe en `Error`.

### Technologies Used
- Rust `std` uniquement — PAS de serde, PAS de tokio, PAS d'ORM
- `#[derive(Debug, Clone, PartialEq)]` sur les types

### Architectural Patterns
- Domain-Driven Design : entites avec identite, value objects immutables
- State machine explicite avec transitions validees
- Domain events pour decouplage (le domaine emet, les adapters reagissent)

## Success Criteria
- [x] `Download::new()` cree un download en etat `Queued`
- [x] `Download::pause()` depuis `Downloading` retourne `Ok(DownloadPaused)`, depuis `Completed` retourne `Err(InvalidTransition)`
- [x] `Download::retry()` incremente `retry_count`, passe en `Retry`, et echoue apres `max_retries`
- [x] Toutes les transitions invalides retournent `DomainError::InvalidTransition`
- [x] Aucun `use` vers un crate externe dans tout `domain/`
- [x] `cargo test domain` — 90%+ coverage, tous les tests passent
- [x] Les value objects (DownloadId, Priority, Speed, FileSize) sont immutables et comparables

## Testing & Validation

### Tests a ecrire (TDD RED d'abord)
```rust
test_download_new_starts_queued
test_download_start_from_queued_succeeds
test_download_pause_from_downloading_succeeds
test_download_pause_from_completed_fails
test_download_resume_from_paused_succeeds
test_download_retry_increments_count
test_download_retry_circuit_breaker_after_max
test_download_complete_from_downloading_succeeds
test_segment_new_starts_pending
test_segment_complete_updates_downloaded_bytes
test_priority_ordering
test_download_state_all_valid_transitions
test_download_state_all_invalid_transitions
```

### Edge Cases
- Retry count a 0 puis exactement max_retries
- Transition depuis chaque etat vers chaque autre etat (matrice complete)
- Download avec 0 segments vs 32 segments

## Dependencies

**Must complete first**: Task 01 (Project Scaffolding)

**Blocks**: Task 04 (Domain Ports), Task 05 (CQRS), Task 06 (SQLite), Task 08 (Engine)

## Related Documentation
- **PRD**: §6.1.2 Etats d'un telechargement, §7.1 Moteur core
- **ARCHI**: Hexagonal Architecture > Domain Layer

---
**Estimated Time**: 2-3 hours
**Phase**: Foundation
