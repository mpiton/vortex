//! Hexagonal architecture ports.
//!
//! **Driven ports** (secondary): traits the domain requires from the outside
//! world — persistence, networking, filesystem, plugins, events.
//!
//! **Driving ports** (primary): traits that define how the outside world
//! enters the application — command and query handlers.

pub mod driven;
pub mod driving;

// Re-export driven ports
pub use driven::{
    ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadReadRepository,
    DownloadRepository, EventBus, FileStorage, HistoryRepository, HttpClient, PluginLoader,
    StatsRepository,
};

// Re-export driving ports
pub use driving::{Command, CommandHandler, Query, QueryHandler};
