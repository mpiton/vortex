//! Driving adapters — entry points that trigger commands and queries.
//!
//! Currently: Tauri IPC handlers.
//! Planned: REST API (axum), CLI headless mode.

pub mod tauri_ipc;
