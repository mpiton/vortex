//! Driven adapters — implementations of domain port traits.

pub mod clipboard;
pub mod config;
pub mod credential;
pub mod event;
pub mod extractor;
pub mod filesystem;
pub mod network;
pub mod notification;
pub mod plugin;
pub mod sqlite;
#[cfg(test)]
pub mod stats;
pub mod tray;
