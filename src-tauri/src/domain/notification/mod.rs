//! Desktop notification domain helpers.
//!
//! Pure formatting + aggregation logic shared by adapters that surface
//! download lifecycle events to the user (`tauri-plugin-notification`,
//! REST WebSocket toast, future tray balloons).
//!
//! Lives in the domain layer because the rules ("aggregate ≥3 within 5s",
//! "format bytes with 1024 base") are policy decisions, not adapter
//! implementation details.

pub mod format;
pub mod grouper;

pub use format::{format_duration, format_size, format_speed};
pub use grouper::{
    GROUPING_THRESHOLD, GROUPING_WINDOW_SECS, NotificationDecision, NotificationGrouper,
};
