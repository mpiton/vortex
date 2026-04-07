mod adapters;
mod application;
mod domain;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
