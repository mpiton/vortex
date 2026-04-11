use tauri::{
    AppHandle, Emitter, Manager,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tracing::{info, warn};

/// Initializes the system tray with menu items.
///
/// Menu structure:
/// - Pause All
/// - Resume All
/// - ─────────
/// - ☑ Clipboard Monitoring
/// - ─────────
/// - Open Window
/// - Quit
pub fn setup_system_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let pause_all = MenuItem::with_id(app, "pause-all", "Pause All", true, None::<&str>)?;
    let resume_all = MenuItem::with_id(app, "resume-all", "Resume All", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let clipboard_toggle = CheckMenuItem::with_id(
        app,
        "clipboard-toggle",
        "Clipboard Monitoring",
        true, // enabled
        true, // checked by default
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
            // Emit a Tauri event that the frontend can listen to
            if let Err(e) = app.emit("tray-pause-all", ()) {
                warn!("Failed to emit pause-all: {e}");
            }
        }
        "resume-all" => {
            if let Err(e) = app.emit("tray-resume-all", ()) {
                warn!("Failed to emit resume-all: {e}");
            }
        }
        "clipboard-toggle" => {
            if let Err(e) = app.emit("tray-clipboard-toggle", ()) {
                warn!("Failed to emit clipboard-toggle: {e}");
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
