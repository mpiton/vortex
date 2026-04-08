//! Domain layer — pure business logic, zero external dependencies.
//!
//! Contains entities, value objects, domain events, domain errors,
//! and port traits (interfaces for the hexagonal architecture).

pub mod error;
pub mod event;
pub mod model;
pub mod ports;

pub use error::DomainError;
pub use event::DomainEvent;
