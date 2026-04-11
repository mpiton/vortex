use tauri::{
    AppHandle, Emitter, Manager,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tracing::{info, warn};

use crate::adapters::driving::tauri_ipc::AppState;

/// Initializes the system tray with menu items.
///
/// `clipboard_enabled` controls the initial checked state of the Clipboard
/// Monitoring checkbox. Pass the value from persisted config when available.
pub fn setup_system_tray(
    app: &tauri::App,
    clipboard_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let pause_all = MenuItem::with_id(app, "pause-all", "Pause All", true, None::<&str>)?;
    let resume_all = MenuItem::with_id(app, "resume-all", "Resume All", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let clipboard_toggle = CheckMenuItem::with_id(
        app,
        "clipboard-toggle",
        "Clipboard Monitoring",
        true,              // menu item enabled
        clipboard_enabled, // initial checked state from config
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let open_window = MenuItem::with_id(app, "open-window", "Open Window", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &pause_all,
            &resume_all,
            &sep1,
            &clipboard_toggle,
            &sep2,
            &open_window,
            &quit,
        ],
    )?;

    let _tray = TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or("app must have a default icon configured in tauri.conf.json")?,
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Vortex Download Manager")
        .on_menu_event(move |app, event| {
            handle_tray_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    info!("System tray initialized");
    Ok(())
}

fn handle_tray_menu_event(app: &AppHandle, menu_id: &str) {
    match menu_id {
        "pause-all" => {
            // Dispatch via AppState if wired; otherwise log that it's not yet available
            if let Some(state) = app.try_state::<AppState>() {
                let cmd = crate::application::commands::PauseAllDownloadsCommand;
                let bus = state.command_bus.clone();
                tokio::spawn(async move {
                    if let Err(e) = bus.handle_pause_all(cmd).await {
                        warn!("Tray pause-all failed: {e}");
                    }
                });
            } else {
                warn!("Tray pause-all: AppState not yet wired");
            }
        }
        "resume-all" => {
            if let Some(state) = app.try_state::<AppState>() {
                let cmd = crate::application::commands::ResumeAllDownloadsCommand;
                let bus = state.command_bus.clone();
                tokio::spawn(async move {
                    if let Err(e) = bus.handle_resume_all(cmd).await {
                        warn!("Tray resume-all failed: {e}");
                    }
                });
            } else {
                warn!("Tray resume-all: AppState not yet wired");
            }
        }
        "clipboard-toggle" => {
            if let Some(state) = app.try_state::<AppState>() {
                // Read current state and toggle
                let current = state
                    .command_bus
                    .config_store()
                    .get_config()
                    .map(|c| c.clipboard_monitoring)
                    .unwrap_or(false);
                let new_state = !current;

                if let Err(e) = state.command_bus.handle_toggle_clipboard(new_state) {
                    warn!("Tray clipboard toggle failed: {e}");
                    return;
                }

                // Notify frontend
                if let Err(e) = app.emit(
                    "clipboard-monitoring-changed",
                    serde_json::json!({ "enabled": new_state }),
                ) {
                    warn!("Tray clipboard toggle event failed: {e}");
                }
            } else {
                warn!("Tray clipboard-toggle: AppState not yet wired");
            }
        }
        "open-window" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "quit" => {
            app.exit(0);
        }
        _ => {
            warn!("Unknown tray menu event: {menu_id}");
        }
    }
}
