//! [`IconSwapper`] implementation backed by a real Tauri [`TrayIcon`].
//!
//! Owns the pre-rendered pulse frames as `Image<'static>` (so the underlying
//! RGBA buffers live as long as the swapper) plus the static fallback icon,
//! and forwards each request from the animator to `TrayIcon::set_icon`.

use tauri::image::Image;
use tauri::tray::TrayIcon;
use tracing::warn;

use crate::adapters::driven::tray::animator::IconSwapper;
use crate::adapters::driven::tray::frames::TrayFrame;

pub struct TauriIconSwapper {
    tray: TrayIcon,
    frames: Vec<Image<'static>>,
    static_icon: Image<'static>,
}

impl TauriIconSwapper {
    /// Builds a swapper from a tray handle, the static fallback icon, and
    /// the animation frames. Returns `None` when no frames are supplied —
    /// the animator should not be spawned in that case.
    pub fn new(
        tray: TrayIcon,
        static_icon: Image<'static>,
        frames: Vec<TrayFrame>,
    ) -> Option<Self> {
        if frames.is_empty() {
            return None;
        }
        let frames = frames
            .into_iter()
            .map(|f| Image::new_owned(f.rgba, f.width, f.height))
            .collect();
        Some(Self {
            tray,
            frames,
            static_icon,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

impl IconSwapper for TauriIconSwapper {
    fn show_frame(&self, frame_index: usize) {
        let frame = &self.frames[frame_index % self.frames.len()];
        if let Err(e) = self.tray.set_icon(Some(frame.clone())) {
            warn!(error = %e, "tray set_icon (animation frame) failed");
        }
    }

    fn show_static(&self) {
        if let Err(e) = self.tray.set_icon(Some(self.static_icon.clone())) {
            warn!(error = %e, "tray set_icon (static) failed");
        }
    }
}
