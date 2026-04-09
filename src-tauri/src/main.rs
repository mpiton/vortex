// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Full AppState wiring (building CommandBus + QueryBus from real adapters)
    // will be implemented in task 16 when the frontend connects.
    //
    // Required: SQLite connection, TokioEventBus, ReqwestHttpClient,
    // FsFileStorage, SegmentedDownloadEngine, and stub impls for PluginLoader,
    // ConfigStore, CredentialStore, ClipboardObserver, StatsRepository.
    unimplemented!(
        "AppState adapter wiring not yet implemented — \
         run `cargo test` for backend validation"
    )
}
