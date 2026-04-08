//! Port for the domain event bus.
//!
//! Publishes domain events and allows subscribers to react.
//! The adapter uses tokio broadcast channels.

use crate::domain::event::DomainEvent;

/// Publishes and subscribes to domain events.
///
/// Events are fire-and-forget from the publisher's perspective.
/// Subscribers (adapters) react asynchronously — e.g., the Tauri
/// bridge forwards events to the frontend, the persistence adapter
/// updates read models.
pub trait EventBus: Send + Sync {
    /// Publish a domain event to all subscribers.
    fn publish(&self, event: DomainEvent);

    /// Register a subscriber that will receive all future events.
    fn subscribe(&self, handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>);
}
