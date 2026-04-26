mod activity_tracker;
mod animator;
mod frames;
mod system_tray;
mod tauri_swapper;

pub use animator::{DEFAULT_FRAME_INTERVAL, IconSwapper, spawn_tray_animator};
pub use frames::pulse_frames;
pub use system_tray::setup_system_tray;
pub use tauri_swapper::TauriIconSwapper;
