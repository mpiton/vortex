//! Driving port for query execution (CQRS read side).
//!
//! Queries read application state without side effects. Each query is
//! handled by exactly one `QueryHandler` implementation.

use crate::domain::error::DomainError;

/// Marker trait for query types.
///
/// A query represents a request for data (e.g., list downloads, get
/// statistics, search plugins). Queries never modify state.
pub trait Query: Send + 'static {}

/// Handles a single query type, producing a read model or an error.
///
/// Implementations live in the application layer and typically read
/// from optimized read repositories, returning DTOs / view types.
///
/// # Type Parameters
/// - `Q`: The query type this handler processes.
pub trait QueryHandler<Q: Query>: Send + Sync {
    /// The read model / DTO produced by this query.
    type Output;

    /// Execute the query and return the result.
    fn handle(
        &self,
        query: Q,
    ) -> impl std::future::Future<Output = Result<Self::Output, DomainError>> + Send;
}
