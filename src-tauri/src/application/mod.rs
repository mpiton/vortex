//! Application layer — orchestration via CQRS command and query handlers.
//!
//! Commands mutate state through domain entities and emit domain events.
//! Queries read from optimized read models (DTOs).

pub mod command_bus;
pub mod commands;
pub mod error;
pub mod queries;
pub mod query_bus;
pub mod read_models;
pub mod services;
#[cfg(test)]
pub(crate) mod test_support;
