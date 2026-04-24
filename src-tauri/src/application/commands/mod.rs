//! CQRS command types and handlers.
//!
//! Each command represents an intent to mutate application state.
//! Handler implementations live in submodules and add methods to `CommandBus`.

mod cancel_download;
mod clear_downloads_by_state;
mod delete_history;
mod export_history;
mod extract_archive;
mod install_plugin;
mod open_download_file;
mod open_download_folder;
mod pause_all;
mod pause_download;
mod purge_history;
mod register_local_file;
mod remove_download;
mod resolve_links;
mod resume_all;
mod resume_download;
mod retry_download;
mod set_priority;
mod start_download;
pub mod store_install;
pub mod store_refresh;
mod toggle_clipboard;
mod toggle_plugin;
mod uninstall_plugin;
mod update_config;
mod verify_checksum;

use std::path::PathBuf;

use crate::domain::model::config::ConfigPatch;
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driving::Command;

#[derive(Debug)]
pub struct StartDownloadCommand {
    pub url: String,
    pub destination: Option<PathBuf>,
    /// Pre-computed filename (e.g. "Rick Astley - Never Gonna Give You Up.mp4").
    /// When set, skips the HEAD probe and URL-fallback derivation.
    pub filename: Option<String>,
    /// Hostname to store in `source_hostname` instead of the one derived from
    /// `url`. Used when `url` is a CDN URL but we want to display the origin
    /// host (e.g. "youtube.com" instead of "rr1---sn-n4g-cvq6.googlevideo.com").
    pub source_hostname_override: Option<String>,
}
impl Command for StartDownloadCommand {}

#[derive(Debug)]
pub struct PauseDownloadCommand {
    pub id: DownloadId,
}
impl Command for PauseDownloadCommand {}

#[derive(Debug)]
pub struct ResumeDownloadCommand {
    pub id: DownloadId,
}
impl Command for ResumeDownloadCommand {}

#[derive(Debug)]
pub struct CancelDownloadCommand {
    pub id: DownloadId,
}
impl Command for CancelDownloadCommand {}

#[derive(Debug)]
pub struct RetryDownloadCommand {
    pub id: DownloadId,
}
impl Command for RetryDownloadCommand {}

#[derive(Debug)]
pub struct PauseAllDownloadsCommand;
impl Command for PauseAllDownloadsCommand {}

#[derive(Debug)]
pub struct ResumeAllDownloadsCommand;
impl Command for ResumeAllDownloadsCommand {}

/// Install a plugin from a pre-parsed manifest.
///
/// The driving adapter (IPC) is responsible for parsing the manifest from the
/// source path before constructing this command.
#[derive(Debug)]
pub struct InstallPluginCommand {
    pub manifest: crate::domain::model::plugin::PluginManifest,
}
impl Command for InstallPluginCommand {}

#[derive(Debug)]
pub struct UninstallPluginCommand {
    pub name: String,
}
impl Command for UninstallPluginCommand {}

#[derive(Debug)]
pub struct EnablePluginCommand {
    pub name: String,
}
impl Command for EnablePluginCommand {}

#[derive(Debug)]
pub struct DisablePluginCommand {
    pub name: String,
}
impl Command for DisablePluginCommand {}

#[derive(Debug)]
pub struct SetPriorityCommand {
    pub id: DownloadId,
    pub priority: u8,
}
impl Command for SetPriorityCommand {}

#[derive(Debug)]
pub struct RemoveDownloadCommand {
    pub id: DownloadId,
    pub delete_files: bool,
}
impl Command for RemoveDownloadCommand {}

#[derive(Debug)]
pub struct ClearDownloadsByStateCommand {
    pub state: crate::domain::model::download::DownloadState,
    pub delete_files: bool,
}
impl Command for ClearDownloadsByStateCommand {}

#[derive(Debug)]
pub struct ResolveLinksCommand {
    pub urls: Vec<String>,
}
impl Command for ResolveLinksCommand {}

pub use resolve_links::ResolvedLinkDto;

// Handler: task 23 (settings)
#[derive(Debug)]
pub struct UpdateConfigCommand {
    pub patch: ConfigPatch,
}
impl Command for UpdateConfigCommand {}

// Handler: task 26 (archive extraction)
#[derive(Debug)]
pub struct ExtractArchiveCommand {
    pub download_id: DownloadId,
    pub password: Option<String>,
    pub dest_dir: Option<PathBuf>,
}
impl Command for ExtractArchiveCommand {}

/// Serialization format for history exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportHistoryFormat {
    Csv,
    Json,
}

/// Write every history entry to `path` using `format`.
#[derive(Debug)]
pub struct ExportHistoryCommand {
    pub format: ExportHistoryFormat,
    pub path: PathBuf,
}
impl Command for ExportHistoryCommand {}

/// Delete a single history entry by its primary key.
#[derive(Debug)]
pub struct DeleteHistoryEntryCommand {
    pub id: u64,
}
impl Command for DeleteHistoryEntryCommand {}

/// Purge every history entry.
#[derive(Debug)]
pub struct ClearHistoryCommand;
impl Command for ClearHistoryCommand {}

/// Purge history entries with `completed_at < before_timestamp` (Unix seconds).
#[derive(Debug)]
pub struct PurgeHistoryCommand {
    pub before_timestamp: u64,
}
impl Command for PurgeHistoryCommand {}

/// Re-run the checksum validation for an existing download.
///
/// Used by the detail panel "Verify checksum" action so the user can re-check
/// integrity after a manual file move or after replacing the file.
#[derive(Debug)]
pub struct VerifyChecksumCommand {
    pub id: DownloadId,
}
impl Command for VerifyChecksumCommand {}

/// Launch a completed download file with the OS default application.
#[derive(Debug)]
pub struct OpenDownloadFileCommand {
    pub id: DownloadId,
}
impl Command for OpenDownloadFileCommand {}

/// Open the folder containing a completed download, selecting the file
/// when the host file manager supports it.
#[derive(Debug)]
pub struct OpenDownloadFolderCommand {
    pub id: DownloadId,
}
impl Command for OpenDownloadFolderCommand {}

pub use verify_checksum::VerifyChecksumOutcome;

/// Register an already-downloaded local file as a Completed download.
///
/// Used after `download_to_file` produces a merged file via yt-dlp.
#[derive(Debug)]
pub struct RegisterLocalFileCommand {
    /// Original source URL (e.g. "https://www.youtube.com/watch?v=...")
    pub source_url: String,
    /// Absolute path where the merged file has been moved by the caller.
    pub destination_path: PathBuf,
    /// Final filename (e.g. "Rick Astley - Never Gonna Give You Up.mp4").
    pub filename: String,
    /// Origin hostname override (e.g. "www.youtube.com").
    pub source_hostname: Option<String>,
    /// File size in bytes.
    pub file_size: u64,
}
impl Command for RegisterLocalFileCommand {}
