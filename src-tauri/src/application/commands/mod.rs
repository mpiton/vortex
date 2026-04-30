//! CQRS command types and handlers.
//!
//! Each command represents an intent to mutate application state.
//! Handler implementations live in submodules and add methods to `CommandBus`.

#[cfg(test)]
pub(crate) mod tests_support;

mod add_account;
mod add_download_to_package;
mod cancel_download;
mod change_directory;
mod clear_downloads_by_state;
mod create_package;
mod delete_account;
mod delete_history;
mod delete_package;
mod export_accounts;
mod export_history;
mod extract_archive;
mod group_playlists;
mod import_accounts;
mod install_plugin;
mod move_package_to_folder;
mod move_queue;
mod open_download_file;
mod open_download_folder;
mod pause_all;
mod pause_download;
mod purge_history;
mod redownload;
mod register_local_file;
mod remove_download;
mod remove_download_from_package;
mod report_broken_plugin;
mod resolve_links;
mod resume_all;
mod resume_download;
mod retry_download;
mod set_package_password;
mod set_package_priority;
mod set_priority;
mod start_download;
pub mod store_install;
pub mod store_refresh;
mod toggle_clipboard;
mod toggle_package_auto_extract;
mod toggle_plugin;
mod uninstall_plugin;
mod update_account;
mod update_config;
mod update_package;
mod update_plugin_config;
mod validate_account;
mod verify_checksum;

use std::path::PathBuf;

use crate::domain::model::account::{AccountId, AccountType};
use crate::domain::model::config::ConfigPatch;
use crate::domain::model::download::DownloadId;
use crate::domain::model::package::{PackageId, PackageSourceType};
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

/// Auto-group resolved playlist links into one [`Package`] per unique
/// `playlist_id`. Re-running with the same id reuses the existing
/// package (PRD-v2 §P1.11).
#[derive(Debug)]
pub struct GroupPlaylistsCommand {
    pub groups: Vec<crate::application::services::PlaylistGroup>,
}
impl Command for GroupPlaylistsCommand {}

// Handler: task 23 (settings)
#[derive(Debug)]
pub struct UpdateConfigCommand {
    pub patch: ConfigPatch,
}
impl Command for UpdateConfigCommand {}

/// Open a pre-filled GitHub issue for a broken plugin.
///
/// The handler looks up the plugin's `repository_url` from its manifest,
/// builds the issue body from the supplied diagnostic context (versions,
/// OS, recent log lines, the URL the user was testing if any) and hands
/// the resulting URL to the [`UrlOpener`](crate::domain::ports::driven::UrlOpener)
/// port. No mutation of plugin state — the command is named `*_report_*`
/// for clarity, but it is effectively a "side-effecting query" on the
/// plugin manifest.
#[derive(Debug)]
pub struct ReportBrokenPluginCommand {
    pub plugin_name: String,
    pub log_lines: Vec<String>,
    pub tested_url: Option<String>,
    /// Vortex version reported in the issue body. Provided by the driving
    /// adapter (typically `env!("CARGO_PKG_VERSION")`) so the application
    /// layer doesn't bake the host crate's metadata into its own logic.
    pub vortex_version: String,
    /// `std::env::consts::OS` (or equivalent) recorded by the caller.
    pub os: String,
    /// Local path to the plugin store cache (`plugin-registry-cache.json`).
    /// When the plugin's manifest does not surface a `repository` field,
    /// the handler falls back to this cache so plugins installed before
    /// the field was required keep working.
    pub store_cache_path: Option<std::path::PathBuf>,
}
impl Command for ReportBrokenPluginCommand {}

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

// ── Accounts ─────────────────────────────────────────────────────────

/// Create a new persisted account and store its password in the
/// account-keyring under the freshly generated [`AccountId`].
///
/// `created_at_ms` is supplied by the caller (Unix epoch milliseconds)
/// so handlers stay deterministic in tests. The driving adapter passes
/// `now()` from the host clock.
#[derive(Debug, Clone)]
pub struct AddAccountCommand {
    pub service_name: String,
    pub username: String,
    pub password: String,
    pub account_type: AccountType,
    pub created_at_ms: u64,
}
impl Command for AddAccountCommand {}

/// Partial-mutation payload for [`UpdateAccountCommand`]. All fields are
/// optional; absent values keep the persisted account unchanged.
///
/// Setting `password = Some(_)` rotates the password in the keyring
/// (the SQLite row never sees the secret). Setting `username = Some(_)`
/// also rotates the keyring entry to keep it keyed by the new username.
#[derive(Debug, Clone, Default)]
pub struct AccountPatch {
    pub username: Option<String>,
    pub password: Option<String>,
    pub account_type: Option<AccountType>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UpdateAccountCommand {
    pub id: AccountId,
    pub patch: AccountPatch,
}
impl Command for UpdateAccountCommand {}

/// Delete the account row and its keyring entry. Idempotent — succeeds
/// even if neither exists.
#[derive(Debug, Clone)]
pub struct DeleteAccountCommand {
    pub id: AccountId,
}
impl Command for DeleteAccountCommand {}

/// Probe the upstream service for `id`'s credentials.
///
/// `now_ms` is supplied by the caller so the handler can deterministically
/// stamp `last_validated` on the account row.
#[derive(Debug, Clone)]
pub struct ValidateAccountCommand {
    pub id: AccountId,
    pub now_ms: u64,
}
impl Command for ValidateAccountCommand {}

/// Caller-friendly view of a [`ValidationOutcome`](
/// crate::domain::ports::driven::ValidationOutcome). Same shape — kept
/// in the application layer so IPC adapters don't have to import the
/// domain port path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationOutcomeDto {
    pub valid: bool,
    pub latency_ms: Option<u64>,
    pub traffic_left: Option<u64>,
    pub traffic_total: Option<u64>,
    pub valid_until: Option<u64>,
    pub error_message: Option<String>,
}

impl From<crate::domain::ports::driven::ValidationOutcome> for ValidationOutcomeDto {
    fn from(o: crate::domain::ports::driven::ValidationOutcome) -> Self {
        Self {
            valid: o.valid,
            latency_ms: o.latency_ms,
            traffic_left: o.traffic_left,
            traffic_total: o.traffic_total,
            valid_until: o.valid_until,
            error_message: o.error_message,
        }
    }
}

/// Encrypt every persisted account into a single bundle and write it
/// to `path`. The passphrase is fed to a PBKDF2 KDF; the resulting
/// blob is opaque and unreadable without the same passphrase.
#[derive(Debug, Clone)]
pub struct ExportAccountsCommand {
    pub path: PathBuf,
    pub passphrase: String,
}
impl Command for ExportAccountsCommand {}

/// Decrypt a bundle previously produced by [`ExportAccountsCommand`]
/// and persist every account it contains. Wrong passphrase or any
/// integrity-check failure aborts the import without inserting a
/// single row.
#[derive(Debug, Clone)]
pub struct ImportAccountsCommand {
    pub path: PathBuf,
    pub passphrase: String,
    pub now_ms: u64,
}
impl Command for ImportAccountsCommand {}

/// Outcome of a successful [`ExportAccountsCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportAccountsOutcome {
    pub path: PathBuf,
    pub count: u32,
}

/// Outcome of a successful [`ImportAccountsCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportAccountsOutcome {
    pub path: PathBuf,
    pub imported: u32,
    /// Entries skipped because a row with the same
    /// `(service_name, username)` pair was already persisted.
    pub skipped_duplicates: u32,
}

// ── Packages ─────────────────────────────────────────────────────────

/// Build and persist a fresh `Package` aggregate. The handler generates
/// a UUID v4 for the new id so callers don't have to coordinate ids
/// across processes.
#[derive(Debug, Clone)]
pub struct CreatePackageCommand {
    pub name: String,
    pub source_type: PackageSourceType,
    pub folder_path: Option<String>,
    pub created_at_ms: u64,
}
impl Command for CreatePackageCommand {}

/// Partial-mutation payload for [`UpdatePackageCommand`]. Every field is
/// optional; absent values keep the persisted package unchanged. Use
/// [`SetPackagePasswordCommand`] to rotate the keyring secret — the
/// password column itself is not in this patch.
#[derive(Debug, Clone, Default)]
pub struct PackagePatch {
    pub name: Option<String>,
    pub folder_path: Option<Option<String>>,
    pub priority: Option<u8>,
    pub auto_extract: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UpdatePackageCommand {
    pub id: PackageId,
    pub patch: PackagePatch,
}
impl Command for UpdatePackageCommand {}

/// Delete a package. When `delete_downloads` is `false`, the FK on
/// each member download is cleared (downloads survive); when `true`,
/// each member is removed via [`RemoveDownloadCommand`] before the
/// package row is dropped.
#[derive(Debug, Clone)]
pub struct DeletePackageCommand {
    pub id: PackageId,
    pub delete_downloads: bool,
}
impl Command for DeletePackageCommand {}

/// Set or clear the archive password for a package. The secret is
/// persisted in the OS keyring; the SQLite column only stores the
/// keyring service key as a marker — never the plaintext password.
#[derive(Debug, Clone)]
pub struct SetPackagePasswordCommand {
    pub id: PackageId,
    /// `Some(secret)` rotates the keyring entry, `None` clears it.
    pub password: Option<String>,
}
impl Command for SetPackagePasswordCommand {}

/// Set the package's scheduling priority and propagate the value to
/// every member download. Each impacted download triggers a
/// `DownloadPrioritySet` event so the queue manager re-evaluates
/// scheduling immediately.
#[derive(Debug, Clone)]
pub struct SetPackagePriorityCommand {
    pub id: PackageId,
    pub priority: u8,
}
impl Command for SetPackagePriorityCommand {}

/// Move every member download to `new_folder` and persist the new
/// folder path on the package itself. Re-uses the per-download move
/// logic so each child emits `DownloadDirectoryChanged`.
#[derive(Debug, Clone)]
pub struct MovePackageToFolderCommand {
    pub id: PackageId,
    pub new_folder: PathBuf,
}
impl Command for MovePackageToFolderCommand {}

/// Toggle the package's `auto_extract` flag.
#[derive(Debug, Clone)]
pub struct TogglePackageAutoExtractCommand {
    pub id: PackageId,
}
impl Command for TogglePackageAutoExtractCommand {}

/// Attach a download to a package (sets the FK on the download row).
/// Idempotent — re-attaching a download already in the package is a
/// no-op.
#[derive(Debug, Clone)]
pub struct AddDownloadToPackageCommand {
    pub package_id: PackageId,
    pub download_id: DownloadId,
}
impl Command for AddDownloadToPackageCommand {}

/// Detach a download from a package (clears the FK on the download
/// row). Idempotent.
#[derive(Debug, Clone)]
pub struct RemoveDownloadFromPackageCommand {
    pub package_id: PackageId,
    pub download_id: DownloadId,
}
impl Command for RemoveDownloadFromPackageCommand {}

/// Per-child move outcome surfaced by `move_package_to_folder` so the
/// frontend can show partial failures alongside successes (mirrors
/// `ChangeDirectoryBulkOutcome`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageMoveOutcome {
    pub moved: Vec<DownloadId>,
    pub failed: Vec<change_directory::ChangeDirectoryFailure>,
}

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
