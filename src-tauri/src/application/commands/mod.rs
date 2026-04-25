//! CQRS command types and handlers.
//!
//! Each command represents an intent to mutate application state.
//! Handler implementations live in submodules and add methods to `CommandBus`.

mod cancel_download;
mod change_directory;
mod clear_downloads_by_state;
mod delete_history;
mod export_history;
mod extract_archive;
mod install_plugin;
mod move_queue;
mod open_download_file;
mod open_download_folder;
mod pause_all;
mod pause_download;
mod purge_history;
mod redownload;
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
mod update_plugin_config;
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

/// Move a download to the top of the queue by giving it the smallest
/// `queue_position` value among all currently Queued/Retry/Waiting items.
#[derive(Debug)]
pub struct MoveToTopCommand {
    pub id: DownloadId,
}
impl Command for MoveToTopCommand {}

/// Move a download to the bottom of the queue by giving it the largest
/// `queue_position` value among all currently Queued/Retry/Waiting items.
#[derive(Debug)]
pub struct MoveToBottomCommand {
    pub id: DownloadId,
}
impl Command for MoveToBottomCommand {}

/// Reorder the queue using an explicit list of download IDs. Positions are
/// reassigned sequentially starting from 1. Downloads not listed keep their
/// current position. Used by the drag & drop reorder UI.
#[derive(Debug)]
pub struct ReorderQueueCommand {
    pub ordered_ids: Vec<DownloadId>,
}
impl Command for ReorderQueueCommand {}

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

/// Update a single (key, value) pair on a plugin's persisted configuration.
///
/// The handler validates the value against the manifest schema before
/// persisting, so the backend remains the source of truth even if a
/// rogue caller bypasses the UI form.
#[derive(Debug)]
pub struct UpdatePluginConfigCommand {
    pub plugin_name: String,
    pub key: String,
    pub value: String,
}
impl Command for UpdatePluginConfigCommand {}

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

/// Source to rebuild a download from when the user triggers "Re-download".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedownloadSource {
    /// Clone a completed download aggregate (URL + options preserved).
    Download(DownloadId),
    /// Rebuild from a history entry (URL/destination only — history does not
    /// retain segments/priority/module/account).
    History(u64),
}

/// Re-create a download using the URL and options of a previous one.
///
/// Always produces a brand-new `DownloadId`. When the destination file
/// already exists, the driving adapter is responsible for prompting the
/// user (overwrite / rename) and passing the resolved path via
/// `destination_override` before invoking the handler.
#[derive(Debug, Clone)]
pub struct RedownloadCommand {
    pub source: RedownloadSource,
    pub destination_override: Option<PathBuf>,
}
impl Command for RedownloadCommand {}

/// Move a single download's on-disk file (and its `.vortex-meta` sidecar
/// when applicable) into a new destination directory. Pauses then resumes
/// the engine when the download is currently running so the move never
/// races against an in-flight write.
#[derive(Debug)]
pub struct ChangeDirectoryCommand {
    pub id: DownloadId,
    /// Absolute path of the destination directory. The handler appends the
    /// existing filename so the on-disk basename never changes.
    pub new_destination_dir: PathBuf,
}
impl Command for ChangeDirectoryCommand {}

/// Move several downloads in one IPC round-trip. Each item is treated as
/// an independent atomic move — partial failures are reported per id.
#[derive(Debug)]
pub struct ChangeDirectoryBulkCommand {
    pub ids: Vec<DownloadId>,
    pub new_destination_dir: PathBuf,
}
impl Command for ChangeDirectoryBulkCommand {}

pub use change_directory::{ChangeDirectoryBulkOutcome, ChangeDirectoryFailure};

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
