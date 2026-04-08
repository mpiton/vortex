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

// Re-exports consumed once handlers are implemented (tasks 11-12).
#[allow(unused_imports)]
pub use command_bus::CommandBus;
#[allow(unused_imports)]
pub use error::AppError;
#[allow(unused_imports)]
pub use query_bus::QueryBus;
