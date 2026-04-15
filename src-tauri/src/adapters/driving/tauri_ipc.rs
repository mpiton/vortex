//! Tauri IPC driving adapter — exposes CQRS commands and queries as Tauri commands.
//!
//! Each function converts IPC parameters into a domain command/query,
//! delegates to CommandBus/QueryBus, and serialises the result for the frontend.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;
use tracing;

use crate::adapters::driven::logging::download_log_store::DownloadLogStore;
use crate::application::command_bus::CommandBus;
use crate::application::commands::store_install::{StoreInstallCommand, StoreUpdateCommand};
use crate::application::commands::{
    CancelDownloadCommand, DisablePluginCommand, EnablePluginCommand, InstallPluginCommand,
    PauseAllDownloadsCommand, PauseDownloadCommand, RemoveDownloadCommand, ResolveLinksCommand,
    ResolvedLinkDto, ResumeAllDownloadsCommand, ResumeDownloadCommand, RetryDownloadCommand,
    SetPriorityCommand, StartDownloadCommand, UninstallPluginCommand, UpdateConfigCommand,
};
use crate::application::error::AppError;
use crate::application::queries::{
    CountDownloadsByStateQuery, GetDownloadDetailQuery, GetDownloadsQuery, ListPluginsQuery,
};
use crate::application::query_bus::QueryBus;
use crate::application::read_models::download_detail_view::DownloadDetailViewDto;
use crate::application::read_models::download_view::DownloadViewDto;
use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;
use crate::application::read_models::plugin_view::PluginViewDto;
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::download::{DownloadId, DownloadState};
use crate::domain::model::views::{DownloadFilter, SortDirection, SortField, SortOrder};

/// Shared application state managed by Tauri.
pub struct AppState {
    pub command_bus: Arc<CommandBus>,
    pub query_bus: Arc<QueryBus>,
    pub download_log_store: Arc<DownloadLogStore>,
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

// --- Plugin Commands ---

#[tauri::command]
pub async fn plugin_install(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let plugin_dir = std::path::PathBuf::from(&path);
    let canonical = plugin_dir
        .canonicalize()
        .map_err(|e| format!("invalid plugin path: {e}"))?;
    let config_dir =
        dirs::config_dir().ok_or_else(|| "cannot determine system config directory".to_string())?;
    let allowed_parent = config_dir.join("vortex").join("plugins");
    std::fs::create_dir_all(&allowed_parent)
        .map_err(|e| format!("cannot create plugins dir: {e}"))?;
    let allowed_parent = allowed_parent
        .canonicalize()
        .map_err(|e| format!("cannot resolve plugins dir: {e}"))?;
    if !canonical.starts_with(&allowed_parent) {
        return Err(format!(
            "plugin path must be under {}",
            allowed_parent.display()
        ));
    }
    let (manifest, _wasm_path) =
        crate::adapters::driven::plugin::manifest::parse_manifest(&canonical)
            .map_err(|e| e.to_string())?;
    let cmd = InstallPluginCommand { manifest };
    state
        .command_bus
        .handle_install_plugin(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_uninstall(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cmd = UninstallPluginCommand { name };
    state
        .command_bus
        .handle_uninstall_plugin(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_enable(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cmd = EnablePluginCommand { name };
    state
        .command_bus
        .handle_enable_plugin(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_disable(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cmd = DisablePluginCommand { name };
    state
        .command_bus
        .handle_disable_plugin(cmd)
        .await
        .map_err(|e| e.to_string())
}

// --- Plugin Store Commands ---

/// Returns the plugin store catalogue from the local cache.
#[tauri::command]
pub async fn plugin_store_list(
    state: State<'_, AppState>,
) -> Result<Vec<PluginStoreEntryDto>, String> {
    let cache = store_cache_path();
    state
        .command_bus
        .handle_store_list(&cache)
        .await
        .map_err(|e| e.to_string())
}

/// Re-fetch the remote registry.toml and update the local cache.
#[tauri::command]
pub async fn plugin_store_refresh(state: State<'_, AppState>) -> Result<(), String> {
    let cache = store_cache_path();
    state
        .command_bus
        .handle_store_refresh(&cache)
        .await
        .map_err(|e| e.to_string())
}

/// Download and install a plugin from the registry by name.
#[tauri::command]
pub async fn plugin_store_install(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cache = store_cache_path();
    state
        .command_bus
        .handle_store_install(StoreInstallCommand { name }, &cache)
        .await
        .map_err(|e| e.to_string())
}

/// Unload the current version and install the latest from the registry.
#[tauri::command]
pub async fn plugin_store_update(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cache = store_cache_path();
    state
        .command_bus
        .handle_store_update(StoreUpdateCommand { name }, &cache)
        .await
        .map_err(|e| e.to_string())
}

fn store_cache_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("vortex")
        .join("plugin-registry-cache.json")
}

// --- Queries ---

#[tauri::command]
pub async fn download_list(
    state: State<'_, AppState>,
    filter_state: Option<String>,
    search: Option<String>,
    sort_field: Option<String>,
    sort_direction: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<DownloadViewDto>, String> {
    let filter = if filter_state.is_some() || search.is_some() {
        Some(DownloadFilter {
            state: filter_state.and_then(|s| parse_download_state(&s)),
            search,
            host: None,
        })
    } else {
        None
    };
    let sort = sort_field.map(|f| SortOrder {
        field: parse_sort_field(&f),
        direction: sort_direction
            .as_deref()
            .map(parse_sort_direction)
            .unwrap_or_default(),
    });
    let query = GetDownloadsQuery {
        filter,
        sort,
        limit,
        offset,
    };
    state
        .query_bus
        .handle_get_downloads(query)
        .await
        .map(|views| views.into_iter().map(DownloadViewDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_detail(
    state: State<'_, AppState>,
    id: u64,
) -> Result<DownloadDetailViewDto, String> {
    let query = GetDownloadDetailQuery { id: DownloadId(id) };
    state
        .query_bus
        .handle_get_download_detail(query)
        .await
        .map(DownloadDetailViewDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_logs(
    state: State<'_, AppState>,
    id: u64,
    limit: usize,
) -> Result<Vec<String>, String> {
    Ok(state.download_log_store.recent(id, limit))
}

#[tauri::command]
pub async fn download_count_by_state(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, usize>, String> {
    state
        .query_bus
        .handle_count_by_state(CountDownloadsByStateQuery)
        .await
        .map(|counts| {
            counts
                .into_iter()
                .map(|(state, count)| (state.to_string(), count))
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_list(state: State<'_, AppState>) -> Result<Vec<PluginViewDto>, String> {
    state
        .query_bus
        .handle_list_plugins(ListPluginsQuery)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn link_resolve(
    state: State<'_, AppState>,
    urls: Vec<String>,
) -> Result<Vec<ResolvedLinkDto>, String> {
    let cmd = ResolveLinksCommand { urls };
    state
        .command_bus
        .handle_resolve_links(cmd)
        .await
        .map_err(|e| match &e {
            AppError::Validation(msg) => msg.clone(),
            other => {
                tracing::error!(error = %other, "link resolution failed");
                "Failed to resolve links".to_string()
            }
        })
}

// ── Clipboard ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn clipboard_toggle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    let result = state
        .command_bus
        .handle_toggle_clipboard(enabled)
        .map_err(|e| e.to_string())?;

    // Notify frontend of state change
    use tauri::Emitter;
    if let Err(e) = app.emit(
        "clipboard-monitoring-changed",
        serde_json::json!({ "enabled": result }),
    ) {
        tracing::warn!("Failed to emit clipboard-monitoring-changed: {e}");
    }

    Ok(result)
}

#[tauri::command]
pub async fn clipboard_state(state: State<'_, AppState>) -> Result<bool, String> {
    let config = state
        .command_bus
        .config_store()
        .get_config()
        .map_err(|e| e.to_string())?;
    Ok(config.clipboard_monitoring)
}

// ── Settings DTOs ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    // General
    pub download_dir: Option<String>,
    pub start_minimized: bool,
    pub notifications_enabled: bool,
    pub auto_extract: bool,
    pub clipboard_monitoring: bool,
    pub sound_enabled: bool,
    pub confirm_delete: bool,
    pub subfolder_per_package: bool,

    // Downloads
    pub max_concurrent_downloads: u32,
    pub max_segments_per_download: u32,
    pub speed_limit_bytes_per_sec: Option<u64>,
    pub max_retries: u32,
    pub retry_delay_seconds: u32,
    pub verify_checksums: bool,
    pub pre_allocate_space: bool,

    // Network
    pub proxy_type: String,
    pub proxy_url: Option<String>,
    pub user_agent: String,
    pub dns_over_https: bool,
    pub connection_timeout_seconds: u32,

    // Remote Access
    pub web_interface_enabled: bool,
    pub web_interface_port: u16,
    pub rest_api_enabled: bool,
    pub api_key: String,
    pub websocket_enabled: bool,

    // Browser Integration
    pub min_file_size_mb: f64,
    pub excluded_domains: Vec<String>,
    pub excluded_extensions: Vec<String>,

    // Appearance
    pub theme: String,
    pub accent_color: String,
    pub compact_mode: bool,
    pub locale: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusBarDto {
    pub free_space_bytes: Option<u64>,
}

impl From<AppConfig> for SettingsDto {
    fn from(c: AppConfig) -> Self {
        Self {
            download_dir: c.download_dir,
            start_minimized: c.start_minimized,
            notifications_enabled: c.notifications_enabled,
            auto_extract: c.auto_extract,
            clipboard_monitoring: c.clipboard_monitoring,
            sound_enabled: c.sound_enabled,
            confirm_delete: c.confirm_delete,
            subfolder_per_package: c.subfolder_per_package,
            max_concurrent_downloads: c.max_concurrent_downloads,
            max_segments_per_download: c.max_segments_per_download,
            speed_limit_bytes_per_sec: c.speed_limit_bytes_per_sec,
            max_retries: c.max_retries,
            retry_delay_seconds: c.retry_delay_seconds,
            verify_checksums: c.verify_checksums,
            pre_allocate_space: c.pre_allocate_space,
            proxy_type: c.proxy_type,
            proxy_url: c.proxy_url,
            user_agent: c.user_agent,
            dns_over_https: c.dns_over_https,
            connection_timeout_seconds: c.connection_timeout_seconds,
            web_interface_enabled: c.web_interface_enabled,
            web_interface_port: c.web_interface_port,
            rest_api_enabled: c.rest_api_enabled,
            api_key: c.api_key,
            websocket_enabled: c.websocket_enabled,
            min_file_size_mb: c.min_file_size_mb,
            excluded_domains: c.excluded_domains,
            excluded_extensions: c.excluded_extensions,
            theme: c.theme,
            accent_color: c.accent_color,
            compact_mode: c.compact_mode,
            locale: c.locale,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatchDto {
    // General
    pub download_dir: Option<Option<String>>,
    pub start_minimized: Option<bool>,
    pub notifications_enabled: Option<bool>,
    pub auto_extract: Option<bool>,
    pub clipboard_monitoring: Option<bool>,
    pub sound_enabled: Option<bool>,
    pub confirm_delete: Option<bool>,
    pub subfolder_per_package: Option<bool>,

    // Downloads
    pub max_concurrent_downloads: Option<u32>,
    pub max_segments_per_download: Option<u32>,
    pub speed_limit_bytes_per_sec: Option<Option<u64>>,
    pub max_retries: Option<u32>,
    pub retry_delay_seconds: Option<u32>,
    pub verify_checksums: Option<bool>,
    pub pre_allocate_space: Option<bool>,

    // Network
    pub proxy_type: Option<String>,
    pub proxy_url: Option<Option<String>>,
    pub user_agent: Option<String>,
    pub dns_over_https: Option<bool>,
    pub connection_timeout_seconds: Option<u32>,

    // Remote Access
    pub web_interface_enabled: Option<bool>,
    pub web_interface_port: Option<u16>,
    pub rest_api_enabled: Option<bool>,
    pub api_key: Option<String>,
    pub websocket_enabled: Option<bool>,

    // Browser Integration
    pub min_file_size_mb: Option<f64>,
    pub excluded_domains: Option<Vec<String>>,
    pub excluded_extensions: Option<Vec<String>>,

    // Appearance
    pub theme: Option<String>,
    pub accent_color: Option<String>,
    pub compact_mode: Option<bool>,
    pub locale: Option<String>,
}

impl From<ConfigPatchDto> for ConfigPatch {
    fn from(d: ConfigPatchDto) -> Self {
        Self {
            download_dir: d.download_dir,
            start_minimized: d.start_minimized,
            notifications_enabled: d.notifications_enabled,
            auto_extract: d.auto_extract,
            clipboard_monitoring: d.clipboard_monitoring,
            sound_enabled: d.sound_enabled,
            confirm_delete: d.confirm_delete,
            subfolder_per_package: d.subfolder_per_package,
            max_concurrent_downloads: d.max_concurrent_downloads,
            max_segments_per_download: d.max_segments_per_download,
            speed_limit_bytes_per_sec: d.speed_limit_bytes_per_sec,
            max_retries: d.max_retries,
            retry_delay_seconds: d.retry_delay_seconds,
            verify_checksums: d.verify_checksums,
            pre_allocate_space: d.pre_allocate_space,
            proxy_type: d.proxy_type,
            proxy_url: d.proxy_url,
            user_agent: d.user_agent,
            dns_over_https: d.dns_over_https,
            connection_timeout_seconds: d.connection_timeout_seconds,
            web_interface_enabled: d.web_interface_enabled,
            web_interface_port: d.web_interface_port,
            rest_api_enabled: d.rest_api_enabled,
            api_key: d.api_key,
            websocket_enabled: d.websocket_enabled,
            min_file_size_mb: d.min_file_size_mb,
            excluded_domains: d.excluded_domains,
            excluded_extensions: d.excluded_extensions,
            theme: d.theme,
            accent_color: d.accent_color,
            compact_mode: d.compact_mode,
            locale: d.locale,
        }
    }
}

// ── Settings IPC Commands ────────────────────────────────────────────

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    state
        .command_bus
        .config_store()
        .get_config()
        .map(SettingsDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn status_bar_get(state: State<'_, AppState>) -> Result<StatusBarDto, String> {
    let config = state
        .command_bus
        .config_store()
        .get_config()
        .map_err(|e| e.to_string())?;

    let free_space_bytes = match status_bar_path(config.download_dir.as_deref()) {
        Some(path) => tokio::task::spawn_blocking(move || read_available_space(&path))
            .await
            .ok()
            .flatten(),
        None => None,
    };

    Ok(StatusBarDto { free_space_bytes })
}

#[tauri::command]
pub async fn settings_update(
    state: State<'_, AppState>,
    patch: ConfigPatchDto,
) -> Result<SettingsDto, String> {
    let cmd = UpdateConfigCommand {
        patch: patch.into(),
    };
    state
        .command_bus
        .handle_update_config(cmd)
        .map(SettingsDto::from)
        .map_err(|e| e.to_string())
}

// ── Helpers ──────────────────────────────────────────────────────────

fn parse_download_state(s: &str) -> Option<DownloadState> {
    match s.to_lowercase().as_str() {
        "queued" => Some(DownloadState::Queued),
        "downloading" => Some(DownloadState::Downloading),
        "paused" => Some(DownloadState::Paused),
        "waiting" => Some(DownloadState::Waiting),
        "retry" => Some(DownloadState::Retry),
        "error" => Some(DownloadState::Error),
        "completed" => Some(DownloadState::Completed),
        "checking" => Some(DownloadState::Checking),
        "extracting" => Some(DownloadState::Extracting),
        _ => None,
    }
}

fn parse_sort_field(s: &str) -> SortField {
    match s.to_lowercase().as_str() {
        "name" | "filename" => SortField::FileName,
        "size" | "filesize" => SortField::FileSize,
        "progress" => SortField::Progress,
        "speed" => SortField::Speed,
        "state" | "status" => SortField::State,
        _ => SortField::CreatedAt,
    }
}

fn parse_sort_direction(s: &str) -> SortDirection {
    match s.to_lowercase().as_str() {
        "desc" | "descending" => SortDirection::Descending,
        _ => SortDirection::Ascending,
    }
}

fn status_bar_path(download_dir: Option<&str>) -> Option<PathBuf> {
    configured_status_bar_path(download_dir)
        .or_else(dirs::download_dir)
        .or_else(|| std::env::current_dir().ok())
        .and_then(resolve_existing_disk_path)
}

fn configured_status_bar_path(download_dir: Option<&str>) -> Option<PathBuf> {
    download_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                Some(path)
            } else if path.exists() {
                std::env::current_dir().ok().map(|cwd| cwd.join(path))
            } else {
                None
            }
        })
}

fn resolve_existing_disk_path(path: PathBuf) -> Option<PathBuf> {
    let mut current = Some(path.as_path());
    while let Some(candidate) = current {
        if candidate.exists() {
            if candidate.is_dir() {
                return Some(candidate.to_path_buf());
            }
            return candidate.parent().map(|p| p.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

#[cfg(unix)]
fn read_available_space(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();

    // SAFETY: `c_path` is a valid NUL-terminated C string and `stat` points to
    // writable memory for the kernel to fill.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }

    // SAFETY: `statvfs` returned success, so the kernel initialized `stat`.
    let stat = unsafe { stat.assume_init() };
    let available = (stat.f_bavail as u128).saturating_mul(stat.f_frsize as u128);
    Some(available.min(u64::MAX as u128) as u64)
}

#[cfg(windows)]
fn read_available_space(path: &Path) -> Option<u64> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let mut available = 0u64;

    // SAFETY: `wide_path` is NUL-terminated and the output pointer is valid for writes.
    let rc = unsafe {
        GetDiskFreeSpaceExW(
            wide_path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if rc == 0 {
        return None;
    }

    Some(available)
}

#[cfg(not(any(unix, windows)))]
fn read_available_space(_: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{configured_status_bar_path, read_available_space, resolve_existing_disk_path};
    use std::path::PathBuf;

    #[test]
    fn configured_status_bar_path_rejects_empty_values() {
        assert!(configured_status_bar_path(Some("   ")).is_none());
    }

    #[test]
    fn configured_status_bar_path_rejects_missing_relative_paths() {
        assert!(configured_status_bar_path(Some("missing-relative-download-dir")).is_none());
    }

    #[test]
    fn configured_status_bar_path_keeps_absolute_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let absolute_path = temp_dir.path().join("downloads");

        let configured =
            configured_status_bar_path(Some(absolute_path.to_str().expect("utf-8 path")));

        assert_eq!(configured, Some(PathBuf::from(&absolute_path)));
    }

    #[test]
    fn resolve_existing_disk_path_returns_existing_path_unchanged() {
        let temp_dir = tempfile::tempdir().expect("temp dir");

        let resolved =
            resolve_existing_disk_path(temp_dir.path().to_path_buf()).expect("resolved path");

        assert_eq!(resolved, temp_dir.path());
    }

    #[test]
    fn resolve_existing_disk_path_uses_existing_parent() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let nested_missing = temp_dir.path().join("missing").join("nested");

        let resolved = resolve_existing_disk_path(nested_missing).expect("resolved path");

        assert_eq!(resolved, temp_dir.path());
    }

    #[test]
    fn resolve_existing_disk_path_returns_parent_dir_for_file_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&file_path, b"").expect("create temp file");

        let resolved = resolve_existing_disk_path(file_path).expect("resolved path");

        assert_eq!(resolved, temp_dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn read_available_space_returns_a_value_for_existing_directories() {
        let temp_dir = tempfile::tempdir().expect("temp dir");

        assert!(read_available_space(temp_dir.path()).is_some());
    }
}
