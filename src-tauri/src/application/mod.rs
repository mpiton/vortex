//! Application layer — orchestration via CQRS command and query handlers.
//!
//! Commands mutate state through domain entities and emit domain events.
//! Queries read from optimized read models (DTOs).
