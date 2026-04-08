//! Tauri IPC driving adapter — exposes CQRS commands as Tauri commands.
//!
//! Each function converts IPC parameters into a domain command,
//! delegates to CommandBus, and serialises the result for the frontend.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::application::command_bus::CommandBus;
use crate::application::commands::{
    CancelDownloadCommand, PauseAllDownloadsCommand, PauseDownloadCommand, RemoveDownloadCommand,
    ResumeAllDownloadsCommand, ResumeDownloadCommand, RetryDownloadCommand, SetPriorityCommand,
    StartDownloadCommand,
};
use crate::application::query_bus::QueryBus;
use crate::domain::model::download::DownloadId;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub command_bus: Arc<CommandBus>,
    pub query_bus: Arc<QueryBus>,
}

#[tauri::command]
pub async fn download_start(
    state: State<'_, AppState>,
    url: String,
    destination: Option<String>,
) -> Result<u64, String> {
    let cmd = StartDownloadCommand {
        url,
        destination: destination.map(PathBuf::from),
    };
    state
        .command_bus
        .handle_start_download(cmd)
        .await
        .map(|id| id.0)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_pause(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    let cmd = PauseDownloadCommand { id: DownloadId(id) };
    state
        .command_bus
        .handle_pause_download(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_resume(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    let cmd = ResumeDownloadCommand { id: DownloadId(id) };
    state
        .command_bus
        .handle_resume_download(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_cancel(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    let cmd = CancelDownloadCommand { id: DownloadId(id) };
    state
        .command_bus
        .handle_cancel_download(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_retry(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    let cmd = RetryDownloadCommand { id: DownloadId(id) };
    state
        .command_bus
        .handle_retry_download(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_pause_all(state: State<'_, AppState>) -> Result<u32, String> {
    state
        .command_bus
        .handle_pause_all(PauseAllDownloadsCommand)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_resume_all(state: State<'_, AppState>) -> Result<u32, String> {
    state
        .command_bus
        .handle_resume_all(ResumeAllDownloadsCommand)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_set_priority(
    state: State<'_, AppState>,
    id: u64,
    priority: u8,
) -> Result<(), String> {
    let cmd = SetPriorityCommand {
        id: DownloadId(id),
        priority,
    };
    state
        .command_bus
        .handle_set_priority(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_remove(
    state: State<'_, AppState>,
    id: u64,
    delete_files: bool,
) -> Result<(), String> {
    let cmd = RemoveDownloadCommand {
        id: DownloadId(id),
        delete_files,
    };
    state
        .command_bus
        .handle_remove_download(cmd)
        .await
        .map_err(|e| e.to_string())
}
