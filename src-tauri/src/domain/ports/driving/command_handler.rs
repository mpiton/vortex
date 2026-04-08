//! Driving port for command execution (CQRS write side).
//!
//! Commands mutate application state. Each command is handled by exactly
//! one `CommandHandler` implementation in the application layer.

use crate::domain::error::DomainError;

/// Marker trait for command types.
///
/// A command represents an intent to change state (e.g., start a download,
/// pause all, install a plugin). Commands are dispatched to their
/// corresponding `CommandHandler`.
pub trait Command: Send + 'static {}

/// Handles a single command type, producing an output or an error.
///
/// Implementations live in the application layer and orchestrate
/// domain logic via driven ports (repositories, engines, etc.).
///
/// # Type Parameters
/// - `C`: The command type this handler processes.
///
// Note: error type is DomainError for now; task 05 will introduce AppError.
pub trait CommandHandler<C: Command>: Send + Sync {
    /// The value produced on successful command execution.
    type Output;

    /// Execute the command and return the result.
    fn handle(
        &self,
        cmd: C,
    ) -> impl std::future::Future<Output = Result<Self::Output, DomainError>> + Send;
}
