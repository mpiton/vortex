use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tracing::warn;

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

/// Subscribes to the EventBus and sends desktop notifications for key events.
pub fn spawn_notification_bridge(app_handle: AppHandle, event_bus: &dyn EventBus) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| match event {
        DomainEvent::DownloadCompleted { id } => {
            if let Err(e) = app_handle
                .notification()
                .builder()
                .title("Download Complete")
                .body(format!("Download #{} finished successfully", id.0))
                .show()
            {
                warn!("Failed to show completion notification: {e}");
            }
        }
        DomainEvent::DownloadFailed { id, error } => {
            if let Err(e) = app_handle
                .notification()
                .builder()
                .title("Download Failed")
                .body(format!("Download #{}: {error}", id.0))
                .show()
            {
                warn!("Failed to show error notification: {e}");
            }
        }
        _ => {}
    }));
}
