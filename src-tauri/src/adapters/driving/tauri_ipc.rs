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
    CancelDownloadCommand, ClearDownloadsByStateCommand, DisablePluginCommand, EnablePluginCommand,
    InstallPluginCommand, PauseAllDownloadsCommand, PauseDownloadCommand, RemoveDownloadCommand,
    ResolveLinksCommand, ResolvedLinkDto, ResumeAllDownloadsCommand, ResumeDownloadCommand,
    RetryDownloadCommand, SetPriorityCommand, StartDownloadCommand, UninstallPluginCommand,
    UpdateConfigCommand,
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
use crate::domain::ports::driven::PluginLoader;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub command_bus: Arc<CommandBus>,
    pub query_bus: Arc<QueryBus>,
    pub download_log_store: Arc<DownloadLogStore>,
    pub plugin_loader: Arc<dyn PluginLoader>,
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
        filename: None,
        source_hostname_override: None,
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

#[tauri::command]
pub async fn download_clear_completed(
    state: State<'_, AppState>,
    delete_files: bool,
) -> Result<u32, String> {
    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files,
    };
    state
        .command_bus
        .handle_clear_downloads_by_state(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_clear_failed(
    state: State<'_, AppState>,
    delete_files: bool,
) -> Result<u32, String> {
    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Error,
        delete_files,
    };
    state
        .command_bus
        .handle_clear_downloads_by_state(cmd)
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
    let cache = store_cache_path()?;
    state
        .command_bus
        .handle_store_list(&cache)
        .await
        .map_err(|e| e.to_string())
}

/// Re-fetch the remote registry.toml and update the local cache.
#[tauri::command]
pub async fn plugin_store_refresh(state: State<'_, AppState>) -> Result<(), String> {
    let cache = store_cache_path()?;
    state
        .command_bus
        .handle_store_refresh(&cache)
        .await
        .map_err(|e| e.to_string())
}

/// Download and install a plugin from the registry by name.
#[tauri::command]
pub async fn plugin_store_install(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cache = store_cache_path()?;
    state
        .command_bus
        .handle_store_install(StoreInstallCommand { name }, &cache)
        .await
        .map_err(|e| e.to_string())
}

/// Unload the current version and install the latest from the registry.
#[tauri::command]
pub async fn plugin_store_update(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cache = store_cache_path()?;
    state
        .command_bus
        .handle_store_update(StoreUpdateCommand { name }, &cache)
        .await
        .map_err(|e| e.to_string())
}

fn store_cache_path() -> Result<std::path::PathBuf, String> {
    dirs::config_dir()
        .ok_or_else(|| "cannot determine config directory — store unavailable".to_string())
        .map(|p| p.join("vortex").join("plugin-registry-cache.json"))
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

// ── Media Download ───────────────────────────────────────────────────

/// Returns `true` when the URL host belongs to a known media streaming
/// platform that requires a WASM plugin to resolve the CDN stream URL.
///
/// Used to surface a clear "install the plugin" error instead of letting
/// the download engine try — and retry — fetching an HTML page.
fn is_known_media_platform(url: &str) -> bool {
    // Extract the host portion of the URL (tolerant of malformed URLs).
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    // Strip "www." prefix for normalised comparison.
    let host = host.strip_prefix("www.").unwrap_or(&host);

    matches!(
        host,
        "youtube.com"
            | "youtu.be"
            | "m.youtube.com"
            | "music.youtube.com"
            | "vimeo.com"
            | "player.vimeo.com"
            | "soundcloud.com"
            | "on.soundcloud.com"
    )
}

/// Start a download for a media URL (YouTube, Vimeo, SoundCloud, etc.) via
/// the appropriate WASM plugin.
///
/// The plugin's `resolve_stream_url` export is called to obtain a direct CDN
/// URL for the requested quality and format. The resulting URL is then handed
/// to the normal download engine. For generic HTTP URLs (claimed by the
/// built-in HTTP module), the URL is used as-is — unless the URL belongs to a
/// known media platform, in which case a "plugin required" error is returned.
///
/// `title` is the human-readable video title (e.g. "Rick Astley - Never Gonna
/// Give You Up"). When provided it is sanitised and used as the filename
/// (appended with `.{format}`), so the saved file has a meaningful name instead
/// of the CDN path segment ("videoplayback").
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDownloadResultDto {
    pub download_ids: Vec<u64>,
}

async fn rollback_media_download_batch(command_bus: Arc<CommandBus>, download_ids: &[u64]) {
    for id in download_ids.iter().rev().copied() {
        if let Err(error) = command_bus
            .handle_cancel_download(CancelDownloadCommand { id: DownloadId(id) })
            .await
        {
            tracing::warn!(
                download_id = id,
                error = %error,
                "failed to roll back partially-started media download batch"
            );
        }
    }
}

#[tauri::command]
pub async fn download_media_start(
    state: State<'_, AppState>,
    url: String,
    quality: String,
    format: String,
    audio_only: bool,
    title: Option<String>,
    playlist_items: Option<Vec<String>>,
) -> Result<MediaDownloadResultDto, String> {
    // Validate the format extension before anything uses it in a filename —
    // `format!("{}.{}", sanitize_filename(title), format)` below would otherwise
    // interpolate attacker-controlled path separators after the last `/` char
    // that `sanitize_filename` already escaped in the stem.
    let format = sanitize_extension(&format)?;
    let url_clone = url.clone();
    let selected_playlist_items = playlist_items.unwrap_or_default();
    let plugin_loader = state.plugin_loader.clone();
    let batch_targets = tokio::task::spawn_blocking(move || {
        load_soundcloud_playlist_download_targets(
            plugin_loader.as_ref(),
            &url_clone,
            &selected_playlist_items,
        )
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))?
    .map_err(|e| e.to_string())?;

    if let Some(targets) = batch_targets {
        let mut download_ids = Vec::with_capacity(targets.len());
        for target in targets {
            let download_title = soundcloud_track_download_title(&target);
            let title = Some(download_title.clone());
            match start_media_download_for_url(
                state.command_bus.clone(),
                state.plugin_loader.clone(),
                target.url,
                quality.clone(),
                format.clone(),
                true,
                title,
            )
            .await
            {
                Ok(id) => download_ids.push(id),
                Err(error) => {
                    rollback_media_download_batch(state.command_bus.clone(), &download_ids).await;
                    return Err(format!(
                        "failed to start batch download for {download_title}: {error}"
                    ));
                }
            }
        }
        return Ok(MediaDownloadResultDto { download_ids });
    }

    let download_id = start_media_download_for_url(
        state.command_bus.clone(),
        state.plugin_loader.clone(),
        url,
        quality,
        format,
        audio_only,
        title,
    )
    .await?;

    Ok(MediaDownloadResultDto {
        download_ids: vec![download_id],
    })
}

/// Remove characters that are invalid in filenames on Linux / macOS / Windows.
/// Replaces `/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|` and null bytes with `_`.
/// Truncates to 200 chars to stay well inside filesystem limits.
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect();
    // Trim leading/trailing dots and spaces (problematic on Windows and visually
    // misleading on any platform), then truncate to a sane length.
    let trimmed = sanitized.trim_matches(|c| c == '.' || c == ' ');
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed.chars().take(200).collect()
    }
}

/// Validate a file extension before splicing it into a filename.
///
/// Rejects anything that isn't purely ASCII alphanumeric (no path separators,
/// no `..`, no NUL). Accepts (and strips) a single leading dot so both `"mp4"`
/// and `".mp4"` are valid input. Returns the normalized lowercase extension
/// without the leading dot.
///
/// Called at the IPC boundary of `download_media_start` because the raw
/// `format` parameter flows into `format!("{title}.{format}")` — a crafted
/// `"../evil"` would otherwise reach `dest_dir.join(&filename)` and escape
/// the download directory.
fn sanitize_extension(ext: &str) -> Result<String, String> {
    let trimmed = ext.trim().trim_start_matches('.');
    if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("invalid format extension: {ext:?}"));
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// If `dir/filename` already exists, probe `filename (1)`, `filename (2)`, …
/// until a free slot is found. Preserves the extension.
///
/// Returns `(path, filename)` both reflecting the final (possibly suffixed) name,
/// so callers that store the chosen filename in the download record stay in sync
/// with what was actually written to disk.
///
/// Errors out after 9999 collisions rather than silently overwriting — that
/// branch is meant to *prevent* overwrites, not fall back to them.
///
/// Race condition note: TOCTOU-safe this is not — another process could create
/// the same path between the `exists()` check and the subsequent
/// `rename`/`copy`. That would result in an overwrite. For downloads, the
/// window is small and the alternative (`O_EXCL`-style create + rename) is not
/// available on `std::fs::rename`. Accepted as a practical compromise.
fn unique_destination(
    dir: &std::path::Path,
    filename: &str,
) -> Result<(std::path::PathBuf, String), String> {
    let base = dir.join(filename);
    if !base.exists() {
        return Ok((base, filename.to_string()));
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());

    for n in 1..=9999 {
        let candidate_name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(&candidate_name);
        if !candidate.exists() {
            return Ok((candidate, candidate_name));
        }
    }

    Err(format!(
        "too many existing files named like {filename:?} in {}",
        dir.display()
    ))
}

/// Extract the hostname from a URL string (e.g. "www.youtube.com" from
/// "https://www.youtube.com/watch?v=..."). Returns `None` when the URL
/// cannot be parsed.
fn extract_hostname_from_url(url: &str) -> Option<String> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ftp://"))?;
    let authority = after_scheme.split('/').next()?;
    // Strip any `user:pass@` userinfo prefix — `rsplit('@').next()` returns
    // the host portion when '@' is present, or the whole string otherwise.
    // Using rsplit (not split) correctly handles passwords that themselves
    // contain '@'.
    let host_and_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_and_port.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[derive(Debug)]
enum StreamResolution {
    CdnUrl(String),
    LocalFile {
        path: std::path::PathBuf,
        size: u64,
        filename: String,
    },
}

async fn start_media_download_for_url(
    command_bus: Arc<CommandBus>,
    plugin_loader: Arc<dyn PluginLoader>,
    url: String,
    quality: String,
    format: String,
    audio_only: bool,
    title: Option<String>,
) -> Result<u64, String> {
    let source_hostname_override = extract_hostname_from_url(&url);
    let url_clone = url.clone();
    let quality_clone = quality.clone();
    let format_clone = format.clone();
    let title_clone = title.clone();

    let resolution = tokio::task::spawn_blocking(move || {
        resolve_media_stream(
            plugin_loader.as_ref(),
            &url_clone,
            &quality_clone,
            &format_clone,
            audio_only,
            title_clone,
        )
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    match resolution {
        StreamResolution::CdnUrl(stream_url) => {
            let filename = title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .map(|t| format!("{}.{}", sanitize_filename(t), format));

            let cmd = crate::application::commands::StartDownloadCommand {
                url: stream_url,
                destination: None,
                filename,
                source_hostname_override,
            };
            command_bus
                .handle_start_download(cmd)
                .await
                .map(|id| id.0)
                .map_err(|e| e.to_string())
        }
        StreamResolution::LocalFile {
            path,
            size,
            filename,
        } => {
            let cmd = crate::application::commands::RegisterLocalFileCommand {
                source_url: url,
                destination_path: path,
                filename,
                source_hostname: source_hostname_override,
                file_size: size,
            };
            command_bus
                .handle_register_local_file(cmd)
                .await
                .map(|id| id.0)
                .map_err(|e| e.to_string())
        }
    }
}

fn resolve_media_stream(
    plugin_loader: &dyn PluginLoader,
    url: &str,
    quality: &str,
    format: &str,
    audio_only: bool,
    title: Option<String>,
) -> Result<StreamResolution, String> {
    match plugin_loader.resolve_stream_url(url, quality, format, audio_only) {
        Ok(cdn_url) => Ok(StreamResolution::CdnUrl(cdn_url)),
        Err(crate::domain::error::DomainError::AdaptiveStreamOnly) => {
            let temp_dir = std::env::temp_dir().join("vortex-downloads");
            std::fs::create_dir_all(&temp_dir)
                .map_err(|e| format!("failed to create temp dir: {e}"))?;

            let file_info = plugin_loader
                .download_to_file(
                    url,
                    quality,
                    format,
                    temp_dir
                        .to_str()
                        .ok_or_else(|| "temp dir path is not valid UTF-8".to_string())?,
                    audio_only,
                )
                .map_err(|e| format!("download_to_file failed: {e}"))?;

            let temp_dir_canonical = temp_dir
                .canonicalize()
                .map_err(|e| format!("failed to canonicalize temp dir: {e}"))?;
            let produced_canonical = file_info
                .path
                .canonicalize()
                .map_err(|e| format!("failed to canonicalize downloaded file path: {e}"))?;
            if !produced_canonical.starts_with(&temp_dir_canonical) {
                return Err(format!(
                    "plugin returned file outside temp dir: {}",
                    file_info.path.display()
                ));
            }

            let filename = title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .map(|t| format!("{}.{}", sanitize_filename(t), format))
                .unwrap_or_else(|| {
                    file_info
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("download")
                        .to_string()
                });

            let dest_dir = dirs::download_dir()
                .or_else(dirs::home_dir)
                .ok_or_else(|| {
                    "cannot determine download destination: neither \
                     user-dirs download_dir nor home_dir are available"
                        .to_string()
                })?;
            std::fs::create_dir_all(&dest_dir).map_err(|e| {
                format!(
                    "failed to create destination dir {}: {e}",
                    dest_dir.display()
                )
            })?;
            let (dest_path, dest_filename) = unique_destination(&dest_dir, &filename)
                .map_err(|e| format!("failed to select unique destination: {e}"))?;

            if std::fs::rename(&file_info.path, &dest_path).is_err() {
                std::fs::copy(&file_info.path, &dest_path)
                    .map_err(|e| format!("failed to copy merged file: {e}"))?;
                if let Err(e) = std::fs::remove_file(&file_info.path) {
                    tracing::warn!(
                        path = %file_info.path.display(),
                        error = %e,
                        "failed to remove temp file after copy"
                    );
                }
            }

            Ok(StreamResolution::LocalFile {
                path: dest_path,
                size: file_info.size,
                filename: dest_filename,
            })
        }
        Err(crate::domain::error::DomainError::NotFound(_)) => {
            if is_known_media_platform(url) {
                Err(
                    "No media plugin installed for this URL. \
                     Open the Plugin Store and install the appropriate plugin (e.g. vortex-mod-youtube)."
                        .to_string(),
                )
            } else {
                Ok(StreamResolution::CdnUrl(url.to_string()))
            }
        }
        Err(e) => Err(format!("Failed to resolve stream URL: {e}")),
    }
}

// ── Media Metadata ───────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadataDto {
    pub title: String,
    pub artist: Option<String>,
    pub thumbnail_url: String,
    pub duration_seconds: u64,
    pub is_playlist: bool,
    pub default_quality: Option<String>,
    pub available_qualities: Vec<QualityOptionDto>,
    pub available_formats: Vec<String>,
    pub available_audio_formats: Vec<String>,
    pub available_subtitles: Vec<SubtitleLanguageDto>,
    pub playlist_items: Option<Vec<PlaylistItemDto>>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOptionDto {
    pub quality: String,
    pub height: u32,
    pub width: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLanguageDto {
    pub code: String,
    pub name: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemDto {
    pub id: String,
    pub title: String,
    pub duration_seconds: u64,
}

#[tauri::command]
pub async fn command_get_media_metadata(
    state: State<'_, AppState>,
    url: String,
) -> Result<MediaMetadataDto, String> {
    let plugin_loader = state.plugin_loader.clone();
    let url_clone = url.clone();
    let plugin_metadata = tokio::task::spawn_blocking(move || {
        load_plugin_media_metadata(plugin_loader.as_ref(), &url_clone)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))?;
    match plugin_metadata {
        Ok(Some(metadata)) => return Ok(metadata),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                url = %url,
                error = %error,
                "plugin metadata extraction failed; falling back to yt-dlp"
            );
        }
    }

    let output = tokio::task::spawn_blocking(move || -> Result<std::process::Output, String> {
        let binary = find_ytdlp()?;
        std::process::Command::new(&binary)
            .args([
                "--dump-single-json",
                "--flat-playlist",
                "--no-warnings",
                &url,
            ])
            .output()
            .map_err(|e| format!("Failed to run yt-dlp: {e}"))
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {stderr}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse yt-dlp output: {e}"))?;

    parse_ytdlp_json(&json)
}

#[derive(Debug, serde::Deserialize)]
struct PluginExtractLinksKind {
    kind: String,
}

#[derive(Debug, serde::Deserialize)]
struct PluginVideoExtractLinksResponse {
    kind: String,
    #[serde(default)]
    videos: Vec<PluginVideoMediaLink>,
}

#[derive(Debug, serde::Deserialize)]
struct PluginVideoMediaLink {
    title: String,
    duration: Option<u64>,
    thumbnail: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PluginMediaVariantsResponse {
    #[serde(default)]
    variants: Vec<PluginMediaVariant>,
}

#[derive(Debug, serde::Deserialize)]
struct PluginMediaVariant {
    kind: PluginVariantKind,
    ext: String,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    abr: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum PluginVariantKind {
    Video,
    Audio,
    Adaptive,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SoundcloudExtractLinksResponse {
    kind: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    #[serde(default)]
    tracks: Vec<SoundcloudTrackLink>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SoundcloudTrackLink {
    id: String,
    title: String,
    url: String,
    artist: Option<String>,
    duration_ms: Option<u64>,
    artwork_url: Option<String>,
}

fn load_plugin_media_metadata(
    plugin_loader: &dyn PluginLoader,
    url: &str,
) -> Result<Option<MediaMetadataDto>, crate::domain::DomainError> {
    let Some(_) = plugin_loader.resolve_url(url)? else {
        return Ok(None);
    };
    let extract_links = match plugin_loader.extract_links(url) {
        Ok(payload) => payload,
        Err(crate::domain::DomainError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let kind: PluginExtractLinksKind = serde_json::from_str(&extract_links).map_err(|e| {
        crate::domain::DomainError::PluginError(format!(
            "Failed to parse plugin extract_links output: {e}"
        ))
    })?;

    match kind.kind.as_str() {
        "video" => {
            let variants = match plugin_loader.get_media_variants(url) {
                Ok(payload) => payload,
                Err(crate::domain::DomainError::NotFound(_)) => return Ok(None),
                Err(error) => return Err(error),
            };
            parse_plugin_video_metadata(&extract_links, &variants)
                .map(Some)
                .map_err(crate::domain::DomainError::PluginError)
        }
        "track" | "playlist" | "artist" => parse_soundcloud_metadata(&extract_links)
            .map(Some)
            .map_err(crate::domain::DomainError::PluginError),
        _ => Ok(None),
    }
}

fn parse_plugin_video_metadata(
    extract_links_json: &str,
    variants_json: &str,
) -> Result<MediaMetadataDto, String> {
    let extract_links: PluginVideoExtractLinksResponse =
        serde_json::from_str(extract_links_json)
            .map_err(|e| format!("Failed to parse plugin extract_links output: {e}"))?;
    if extract_links.kind != "video" {
        return Err(format!(
            "plugin returned unsupported extract_links kind: {}",
            extract_links.kind
        ));
    }
    let video = extract_links
        .videos
        .into_iter()
        .next()
        .ok_or_else(|| "plugin returned no video entries".to_string())?;

    let variants: PluginMediaVariantsResponse = serde_json::from_str(variants_json)
        .map_err(|e| format!("Failed to parse plugin get_media_variants output: {e}"))?;

    let mut available_qualities = Vec::new();
    let mut available_formats = Vec::new();
    let mut available_audio_formats = Vec::new();
    let mut seen_heights = std::collections::HashSet::<u32>::new();
    let mut seen_video_exts = std::collections::HashSet::<String>::new();
    let mut seen_audio_exts = std::collections::HashSet::<String>::new();

    for variant in variants.variants {
        match variant.kind {
            kind @ (PluginVariantKind::Video | PluginVariantKind::Adaptive) => {
                if matches!(kind, PluginVariantKind::Adaptive) {
                    tracing::warn!(
                        ext = %variant.ext,
                        height = ?variant.height,
                        "surfacing Adaptive plugin variant via merged-download fallback",
                    );
                }
                if let Some(height) = variant.height.filter(|height| *height > 0)
                    && seen_heights.insert(height)
                {
                    available_qualities.push(QualityOptionDto {
                        quality: format!("{height}p"),
                        height,
                        width: variant.width.unwrap_or(0),
                        fps: variant.fps.unwrap_or(0.0).round() as u32,
                        bitrate_kbps: variant.abr.unwrap_or(0.0).round() as u32,
                    });
                }

                if !variant.ext.is_empty() && seen_video_exts.insert(variant.ext.clone()) {
                    available_formats.push(variant.ext);
                }
            }
            PluginVariantKind::Audio => {
                if !variant.ext.is_empty() && seen_audio_exts.insert(variant.ext.clone()) {
                    available_audio_formats.push(variant.ext);
                }
            }
        }
    }

    available_qualities.sort_by_key(|quality| std::cmp::Reverse(quality.height));
    // Pick default_quality from the sorted top so UI and default agree.
    let default_quality = available_qualities.first().map(|q| q.quality.clone());

    Ok(MediaMetadataDto {
        title: video.title,
        artist: None,
        thumbnail_url: video.thumbnail.unwrap_or_default(),
        duration_seconds: video.duration.unwrap_or_default(),
        is_playlist: false,
        default_quality,
        available_qualities,
        available_formats,
        available_audio_formats,
        available_subtitles: Vec::new(),
        playlist_items: None,
    })
}

fn parse_soundcloud_metadata(extract_links_json: &str) -> Result<MediaMetadataDto, String> {
    let extract_links: SoundcloudExtractLinksResponse = serde_json::from_str(extract_links_json)
        .map_err(|e| format!("Failed to parse SoundCloud extract_links output: {e}"))?;

    match extract_links.kind.as_str() {
        "track" => {
            let track = extract_links
                .tracks
                .into_iter()
                .next()
                .ok_or_else(|| "plugin returned no SoundCloud track entries".to_string())?;

            Ok(MediaMetadataDto {
                title: extract_links.title.unwrap_or(track.title),
                artist: track.artist.or(extract_links.artist),
                thumbnail_url: extract_links
                    .artwork_url
                    .or(track.artwork_url)
                    .unwrap_or_default(),
                duration_seconds: millis_to_seconds(track.duration_ms),
                is_playlist: false,
                default_quality: None,
                available_qualities: Vec::new(),
                available_formats: Vec::new(),
                available_audio_formats: vec!["mp3".to_string()],
                available_subtitles: Vec::new(),
                playlist_items: None,
            })
        }
        "playlist" | "artist" => {
            if extract_links.tracks.is_empty() {
                return Err("plugin returned no SoundCloud collection entries".to_string());
            }

            let total_duration_ms: u64 = extract_links
                .tracks
                .iter()
                .filter_map(|track| track.duration_ms)
                .sum();
            let playlist_items = extract_links
                .tracks
                .iter()
                .map(|track| PlaylistItemDto {
                    id: track.id.clone(),
                    title: soundcloud_track_download_title(track),
                    duration_seconds: millis_to_seconds(track.duration_ms),
                })
                .collect::<Vec<_>>();

            let collection_title =
                extract_links
                    .title
                    .unwrap_or_else(|| match extract_links.kind.as_str() {
                        "artist" => format!("SoundCloud artist ({})", extract_links.tracks.len()),
                        _ => format!("SoundCloud playlist ({})", extract_links.tracks.len()),
                    });

            Ok(MediaMetadataDto {
                title: collection_title,
                artist: None,
                thumbnail_url: extract_links.artwork_url.unwrap_or_else(|| {
                    extract_links
                        .tracks
                        .iter()
                        .find_map(|track| track.artwork_url.clone())
                        .unwrap_or_default()
                }),
                duration_seconds: total_duration_ms / 1000,
                is_playlist: true,
                default_quality: None,
                available_qualities: Vec::new(),
                available_formats: Vec::new(),
                available_audio_formats: vec!["mp3".to_string()],
                available_subtitles: Vec::new(),
                playlist_items: Some(playlist_items),
            })
        }
        other => Err(format!(
            "unsupported SoundCloud extract_links kind: {other}"
        )),
    }
}

fn load_soundcloud_playlist_download_targets(
    plugin_loader: &dyn PluginLoader,
    url: &str,
    selected_item_ids: &[String],
) -> Result<Option<Vec<SoundcloudTrackLink>>, crate::domain::DomainError> {
    let Some(info) = plugin_loader.resolve_url(url)? else {
        return Ok(None);
    };
    if info.name() != "vortex-mod-soundcloud" {
        return Ok(None);
    }

    let extract_links = plugin_loader.extract_links(url)?;
    let tracks = parse_soundcloud_playlist_targets(&extract_links, selected_item_ids)
        .map_err(crate::domain::DomainError::PluginError)?;
    if tracks.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tracks))
    }
}

fn parse_soundcloud_playlist_targets(
    extract_links_json: &str,
    selected_item_ids: &[String],
) -> Result<Vec<SoundcloudTrackLink>, String> {
    let extract_links: SoundcloudExtractLinksResponse = serde_json::from_str(extract_links_json)
        .map_err(|e| format!("Failed to parse SoundCloud extract_links output: {e}"))?;

    if extract_links.kind != "playlist" && extract_links.kind != "artist" {
        return Ok(Vec::new());
    }

    let selected_ids: std::collections::HashSet<_> = selected_item_ids.iter().cloned().collect();
    let tracks = extract_links
        .tracks
        .into_iter()
        .filter(|track| selected_ids.is_empty() || selected_ids.contains(&track.id))
        .collect::<Vec<_>>();

    if tracks.is_empty() {
        return Err("no SoundCloud tracks matched the selected playlist items".to_string());
    }

    Ok(tracks)
}

fn soundcloud_track_download_title(track: &SoundcloudTrackLink) -> String {
    match track
        .artist
        .as_deref()
        .filter(|artist| !artist.trim().is_empty())
    {
        Some(artist) => format!("{artist} - {}", track.title),
        None => track.title.clone(),
    }
}

fn millis_to_seconds(duration_ms: Option<u64>) -> u64 {
    duration_ms.unwrap_or_default() / 1000
}

fn find_ytdlp() -> Result<std::path::PathBuf, String> {
    // Try PATH via `which` equivalent — just attempt running `yt-dlp --version`
    if std::process::Command::new("yt-dlp")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok(std::path::PathBuf::from("yt-dlp"));
    }

    // Known fallback locations
    let mut candidates = vec![
        std::path::PathBuf::from("/usr/local/bin/yt-dlp"),
        std::path::PathBuf::from("/usr/bin/yt-dlp"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.insert(0, home.join(".local/bin/yt-dlp"));
    }

    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    Err("yt-dlp not found — install it with: pip install yt-dlp".to_string())
}

/// Canonical YouTube vertical-resolution ladder supported by
/// `vortex-mod-youtube`. Kept in sync with the `default_quality.options` array
/// in the plugin's `plugin.toml`. Anything off this list either has no
/// pre-merged HTTPS stream (yt-dlp fails with "Requested format is not
/// available") or is a non-standard encode the plugin's quality selector
/// cannot target.
const SUPPORTED_YOUTUBE_HEIGHTS: &[u32] = &[360, 480, 720, 1080, 1440, 2160, 4320];

fn is_supported_youtube_height(height: u32) -> bool {
    SUPPORTED_YOUTUBE_HEIGHTS.contains(&height)
}

/// Detect whether yt-dlp's `--dump-single-json` payload describes a YouTube
/// source. The canonical-ladder filter only applies to YouTube; Vimeo,
/// SoundCloud and other extractors expose their own resolution sets (Vimeo
/// for instance serves 540p, which would be wrongly dropped).
///
/// yt-dlp always sets `extractor_key` (e.g. `"Youtube"`, `"Vimeo"`,
/// `"Soundcloud"`); we also check `webpage_url_domain` as a belt-and-braces
/// fallback in case a future yt-dlp release renames the extractor.
fn is_youtube_source(json: &serde_json::Value) -> bool {
    let extractor_key = json["extractor_key"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extractor_key.contains("youtube") {
        return true;
    }
    let webpage_domain = json["webpage_url_domain"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        webpage_domain.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be"
    )
}

fn parse_ytdlp_json(json: &serde_json::Value) -> Result<MediaMetadataDto, String> {
    let title = json["title"].as_str().unwrap_or("").to_string();
    let thumbnail_url = json["thumbnail"].as_str().unwrap_or("").to_string();
    let duration_seconds = json["duration"].as_f64().unwrap_or(0.0) as u64;
    let is_playlist = json["_type"].as_str() == Some("playlist");

    let mut available_qualities: Vec<QualityOptionDto> = Vec::new();
    let mut seen_heights = std::collections::HashSet::<u32>::new();
    let mut seen_video_exts = std::collections::HashSet::<String>::new();
    let mut seen_audio_exts = std::collections::HashSet::<String>::new();
    let mut available_formats: Vec<String> = Vec::new();
    let mut available_audio_formats: Vec<String> = Vec::new();

    if let Some(formats) = json["formats"].as_array() {
        // Build quality list: deduplicated by height, sorted highest first.
        // For YouTube sources only, restrict heights to the set declared in
        // vortex-mod-youtube's `plugin.toml :: default_quality.options`;
        // anything else (144p, 240p, non-standard heights like 270/1072)
        // would fail in the plugin's `resolve_stream_url`. Other extractors
        // (Vimeo, SoundCloud, …) keep every positive height they report —
        // they have their own resolution sets (Vimeo for instance serves
        // 540p, which is not on the YouTube ladder).
        let youtube_only = is_youtube_source(json);
        let mut video_formats: Vec<&serde_json::Value> = formats
            .iter()
            .filter(|f| f["vcodec"].as_str().unwrap_or("none") != "none")
            .filter(|f| {
                let height = f["height"].as_u64().unwrap_or(0) as u32;
                height > 0 && (!youtube_only || is_supported_youtube_height(height))
            })
            .collect();
        video_formats.sort_by(|a, b| {
            b["height"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["height"].as_u64().unwrap_or(0))
        });
        for f in video_formats {
            let height = f["height"].as_u64().unwrap_or(0) as u32;
            if seen_heights.insert(height) {
                available_qualities.push(QualityOptionDto {
                    quality: format!("{height}p"),
                    height,
                    width: f["width"].as_u64().unwrap_or(0) as u32,
                    fps: f["fps"].as_f64().unwrap_or(0.0) as u32,
                    bitrate_kbps: f["tbr"].as_f64().unwrap_or(0.0) as u32,
                });
            }
        }

        // Video container formats (extensions from video-bearing streams)
        for f in formats.iter() {
            if f["vcodec"].as_str().unwrap_or("none") != "none"
                && let Some(ext) = f["ext"].as_str()
                && seen_video_exts.insert(ext.to_string())
            {
                available_formats.push(ext.to_string());
            }
        }

        // Audio-only formats
        for f in formats.iter() {
            let vcodec = f["vcodec"].as_str().unwrap_or("none");
            let acodec = f["acodec"].as_str().unwrap_or("none");
            if vcodec == "none"
                && acodec != "none"
                && let Some(ext) = f["ext"].as_str()
                && seen_audio_exts.insert(ext.to_string())
            {
                available_audio_formats.push(ext.to_string());
            }
        }
    }

    // Subtitles — keys are BCP-47 language codes
    let available_subtitles: Vec<SubtitleLanguageDto> = json["subtitles"]
        .as_object()
        .map(|subs| {
            subs.keys()
                .filter(|code| *code != "live_chat")
                .map(|code| SubtitleLanguageDto {
                    code: code.clone(),
                    name: code.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    // Playlist entries (only present when _type == "playlist")
    let playlist_items: Option<Vec<PlaylistItemDto>> = if is_playlist {
        json["entries"].as_array().map(|entries| {
            entries
                .iter()
                .map(|e| PlaylistItemDto {
                    id: e["id"].as_str().unwrap_or("").to_string(),
                    title: e["title"].as_str().unwrap_or("").to_string(),
                    duration_seconds: e["duration"].as_f64().unwrap_or(0.0) as u64,
                })
                .collect()
        })
    } else {
        None
    };

    Ok(MediaMetadataDto {
        title,
        artist: None,
        thumbnail_url,
        duration_seconds,
        is_playlist,
        default_quality: None,
        available_qualities,
        available_formats,
        available_audio_formats,
        available_subtitles,
        playlist_items,
    })
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
    use super::{
        configured_status_bar_path, extract_hostname_from_url, load_plugin_media_metadata,
        parse_plugin_video_metadata, parse_soundcloud_metadata, parse_soundcloud_playlist_targets,
        read_available_space, resolve_existing_disk_path, sanitize_extension, sanitize_filename,
        soundcloud_track_download_title, unique_destination,
    };
    use crate::domain::error::DomainError;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use crate::domain::ports::driven::PluginLoader;
    use std::path::PathBuf;

    #[derive(Clone)]
    struct MetadataPluginLoader {
        resolved: Option<PluginInfo>,
        extract_links: Result<String, DomainError>,
        variants: Result<String, DomainError>,
    }

    impl PluginLoader for MetadataPluginLoader {
        fn load(&self, _manifest: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }

        fn unload(&self, _name: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(self.resolved.clone())
        }

        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(Vec::new())
        }

        fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }

        fn extract_links(&self, _url: &str) -> Result<String, DomainError> {
            self.extract_links.clone()
        }

        fn get_media_variants(&self, _url: &str) -> Result<String, DomainError> {
            self.variants.clone()
        }
    }

    fn make_plugin_info(name: &str) -> PluginInfo {
        PluginInfo::new(
            name.to_string(),
            "1.0.0".to_string(),
            "Test plugin".to_string(),
            "tester".to_string(),
            PluginCategory::Utility,
        )
    }

    // ── sanitize_filename ─────────────────────────────────────────────────────

    #[test]
    fn sanitize_filename_replaces_path_separators() {
        assert_eq!(
            sanitize_filename("AC/DC - Back in Black"),
            "AC_DC - Back in Black"
        );
    }

    #[test]
    fn sanitize_filename_replaces_colon() {
        // ":" is common in video titles (e.g. "Part 1: Introduction")
        assert_eq!(
            sanitize_filename("Tutorial: Getting Started"),
            "Tutorial_ Getting Started"
        );
    }

    #[test]
    fn sanitize_filename_replaces_all_invalid_chars() {
        assert_eq!(
            sanitize_filename(r#"a/b\c:d*e?f"g<h>i|j"#),
            "a_b_c_d_e_f_g_h_i_j"
        );
    }

    #[test]
    fn sanitize_filename_trims_leading_trailing_dots() {
        assert_eq!(sanitize_filename("..video.."), "video");
    }

    #[test]
    fn sanitize_filename_trims_leading_trailing_spaces() {
        assert_eq!(sanitize_filename("  video  "), "video");
    }

    #[test]
    fn sanitize_filename_returns_download_for_empty_result() {
        assert_eq!(sanitize_filename("..."), "download");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn sanitize_filename_truncates_long_names() {
        let long = "a".repeat(300);
        assert_eq!(sanitize_filename(&long).len(), 200);
    }

    // ── extract_hostname_from_url ─────────────────────────────────────────────

    #[test]
    fn extract_hostname_from_youtube_url() {
        assert_eq!(
            extract_hostname_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("www.youtube.com".to_string())
        );
    }

    #[test]
    fn extract_hostname_from_cdn_url() {
        assert_eq!(
            extract_hostname_from_url(
                "https://rr1---sn-n4g-cvq6.googlevideo.com/videoplayback?expire=123"
            ),
            Some("rr1---sn-n4g-cvq6.googlevideo.com".to_string())
        );
    }

    #[test]
    fn extract_hostname_from_url_with_port() {
        assert_eq!(
            extract_hostname_from_url("https://example.com:8080/path"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn extract_hostname_returns_none_for_non_url() {
        assert_eq!(extract_hostname_from_url("not-a-url"), None);
    }

    // ── unique_destination ────────────────────────────────────────────────────

    #[test]
    fn unique_destination_returns_original_when_free() {
        let dir = tempfile::tempdir().unwrap();
        let (path, name) = unique_destination(dir.path(), "video.mp4").unwrap();
        assert_eq!(name, "video.mp4");
        assert_eq!(path, dir.path().join("video.mp4"));
    }

    #[test]
    fn unique_destination_suffixes_when_colliding() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("video.mp4"), b"x").unwrap();
        let (path, name) = unique_destination(dir.path(), "video.mp4").unwrap();
        assert_eq!(name, "video (1).mp4");
        assert_eq!(path, dir.path().join("video (1).mp4"));
    }

    #[test]
    fn unique_destination_increments_until_free() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("video.mp4"), b"x").unwrap();
        std::fs::write(dir.path().join("video (1).mp4"), b"x").unwrap();
        std::fs::write(dir.path().join("video (2).mp4"), b"x").unwrap();
        let (_, name) = unique_destination(dir.path(), "video.mp4").unwrap();
        assert_eq!(name, "video (3).mp4");
    }

    #[test]
    fn unique_destination_preserves_dotless_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"x").unwrap();
        let (_, name) = unique_destination(dir.path(), "README").unwrap();
        assert_eq!(name, "README (1)");
    }

    // ── sanitize_extension ────────────────────────────────────────────────────

    #[test]
    fn sanitize_extension_accepts_common_media_extensions() {
        assert_eq!(sanitize_extension("mp4").unwrap(), "mp4");
        assert_eq!(sanitize_extension("webm").unwrap(), "webm");
        assert_eq!(sanitize_extension("m4a").unwrap(), "m4a");
    }

    #[test]
    fn sanitize_extension_strips_leading_dot_and_lowercases() {
        assert_eq!(sanitize_extension(".MP4").unwrap(), "mp4");
        assert_eq!(sanitize_extension(" WebM ").unwrap(), "webm");
    }

    #[test]
    fn sanitize_extension_rejects_empty() {
        assert!(sanitize_extension("").is_err());
        assert!(sanitize_extension(".").is_err());
        assert!(sanitize_extension("  ").is_err());
    }

    #[test]
    fn sanitize_extension_rejects_path_traversal() {
        // The attack vectors this guard exists to stop.
        assert!(sanitize_extension("../etc/passwd").is_err());
        assert!(sanitize_extension("mp4/").is_err());
        assert!(sanitize_extension("mp4\\evil").is_err());
        assert!(sanitize_extension("mp4\0").is_err());
        assert!(sanitize_extension("mp 4").is_err()); // spaces mid-ext
        assert!(sanitize_extension("mp.4").is_err()); // embedded dot
    }

    #[test]
    fn extract_hostname_strips_userinfo() {
        // RFC 3986 allows `user:pass@host` in authority; the original split-on-':'
        // logic returned "user" here. rsplit('@') recovers the real host.
        assert_eq!(
            extract_hostname_from_url("https://user:pass@example.com/path"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_hostname_from_url("https://user@example.com:8080/"),
            Some("example.com".to_string())
        );
        // Password containing '@' must not split the host — rsplit handles this.
        assert_eq!(
            extract_hostname_from_url("https://user:p@ss@example.com/"),
            Some("example.com".to_string())
        );
    }

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

    #[test]
    fn test_parse_plugin_video_metadata_selects_highest_quality_as_default() {
        let extract_links = serde_json::json!({
            "kind": "video",
            "videos": [{
                "title": "Vimeo Plugin Video",
                "duration": 91,
                "thumbnail": "https://example.com/thumb.jpg"
            }]
        });
        let variants = serde_json::json!({
            "variants": [
                {
                    "kind": "video",
                    "ext": "mp4",
                    "width": 1280,
                    "height": 720,
                    "fps": 30.0,
                    "abr": 2400.0
                },
                {
                    "kind": "video",
                    "ext": "mp4",
                    "width": 1920,
                    "height": 1080,
                    "fps": 30.0,
                    "abr": 4200.0
                },
                {
                    "kind": "video",
                    "ext": "mp4",
                    "width": 640,
                    "height": 360,
                    "fps": 30.0,
                    "abr": 900.0
                }
            ]
        });

        let metadata =
            parse_plugin_video_metadata(&extract_links.to_string(), &variants.to_string())
                .expect("plugin metadata should parse");

        assert_eq!(metadata.title, "Vimeo Plugin Video");
        assert_eq!(metadata.default_quality.as_deref(), Some("1080p"));
        let heights: Vec<u32> = metadata
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert_eq!(heights, vec![1080, 720, 360]);
        assert_eq!(metadata.available_formats, vec!["mp4"]);
    }

    #[test]
    fn test_parse_plugin_video_metadata_rejects_non_video_kind() {
        let extract_links = serde_json::json!({
            "kind": "playlist",
            "videos": []
        });
        let variants = serde_json::json!({ "variants": [] });

        let err = parse_plugin_video_metadata(&extract_links.to_string(), &variants.to_string())
            .unwrap_err();

        assert!(err.contains("unsupported extract_links kind"));
    }

    #[test]
    fn test_parse_plugin_video_metadata_keeps_adaptive_variants_available() {
        let extract_links = serde_json::json!({
            "kind": "video",
            "videos": [{
                "title": "Adaptive Vimeo",
                "duration": 91,
                "thumbnail": "https://example.com/thumb.jpg"
            }]
        });
        let variants = serde_json::json!({
            "variants": [
                {
                    "kind": "adaptive",
                    "ext": "mp4",
                    "width": 1280,
                    "height": 720,
                    "fps": 30.0,
                    "abr": 2400.0
                }
            ]
        });

        let metadata =
            parse_plugin_video_metadata(&extract_links.to_string(), &variants.to_string())
                .expect("adaptive metadata should parse");

        assert_eq!(metadata.default_quality.as_deref(), Some("720p"));
        assert_eq!(metadata.available_qualities.len(), 1);
        assert_eq!(metadata.available_qualities[0].quality, "720p");
        assert_eq!(metadata.available_formats, vec!["mp4"]);
    }

    #[test]
    fn test_load_plugin_media_metadata_dispatches_by_extract_links_kind() {
        let extract_links = serde_json::json!({
            "kind": "video",
            "videos": [{
                "title": "Future Provider",
                "duration": 45,
                "thumbnail": "https://example.com/future.jpg"
            }]
        });
        let variants = serde_json::json!({
            "variants": [
                {
                    "kind": "video",
                    "ext": "mp4",
                    "width": 1920,
                    "height": 1080,
                    "fps": 30.0,
                    "abr": 3200.0
                }
            ]
        });
        let loader = MetadataPluginLoader {
            resolved: Some(make_plugin_info("future-video-plugin")),
            extract_links: Ok(extract_links.to_string()),
            variants: Ok(variants.to_string()),
        };

        let metadata = load_plugin_media_metadata(&loader, "https://example.com/video")
            .expect("metadata lookup should succeed")
            .expect("video metadata should be returned");

        assert_eq!(metadata.title, "Future Provider");
        assert_eq!(metadata.default_quality.as_deref(), Some("1080p"));
    }

    #[test]
    fn test_load_plugin_media_metadata_returns_none_when_variants_export_missing() {
        let extract_links = serde_json::json!({
            "kind": "video",
            "videos": [{
                "title": "Video Without Variants",
                "duration": 45,
                "thumbnail": "https://example.com/future.jpg"
            }]
        });
        let loader = MetadataPluginLoader {
            resolved: Some(make_plugin_info("video-without-variants")),
            extract_links: Ok(extract_links.to_string()),
            variants: Err(DomainError::NotFound(
                "get_media_variants not supported by this loader".to_string(),
            )),
        };

        let metadata = load_plugin_media_metadata(&loader, "https://example.com/video")
            .expect("missing exports should fall back cleanly");

        assert!(metadata.is_none());
    }

    #[test]
    fn test_parse_soundcloud_metadata_for_track() {
        let extract_links = serde_json::json!({
            "kind": "track",
            "title": "Flickermood",
            "artist": "Forss",
            "artwork_url": "https://i1.sndcdn.com/artworks-12345-t500x500.jpg",
            "tracks": [{
                "id": "123",
                "title": "Flickermood",
                "url": "https://soundcloud.com/forss/flickermood",
                "artist": "Forss",
                "duration_ms": 225000,
                "artwork_url": "https://i1.sndcdn.com/artworks-12345-t500x500.jpg"
            }]
        });

        let metadata = parse_soundcloud_metadata(&extract_links.to_string())
            .expect("SoundCloud track metadata should parse");

        assert_eq!(metadata.title, "Flickermood");
        assert_eq!(metadata.artist.as_deref(), Some("Forss"));
        assert_eq!(metadata.duration_seconds, 225);
        assert_eq!(metadata.available_audio_formats, vec!["mp3"]);
        assert!(!metadata.is_playlist);
        assert!(metadata.playlist_items.is_none());
    }

    #[test]
    fn test_parse_soundcloud_metadata_for_playlist() {
        let extract_links = serde_json::json!({
            "kind": "playlist",
            "title": "Soulhack",
            "tracks": [
                {
                    "id": "123",
                    "title": "Flickermood",
                    "url": "https://soundcloud.com/forss/flickermood",
                    "artist": "Forss",
                    "duration_ms": 225000,
                    "artwork_url": "https://i1.sndcdn.com/artworks-12345-t500x500.jpg"
                },
                {
                    "id": "124",
                    "title": "Journeyman",
                    "url": "https://soundcloud.com/forss/journeyman",
                    "artist": "Forss",
                    "duration_ms": 180000,
                    "artwork_url": null
                }
            ]
        });

        let metadata = parse_soundcloud_metadata(&extract_links.to_string())
            .expect("SoundCloud playlist metadata should parse");

        assert!(metadata.is_playlist);
        assert_eq!(metadata.title, "Soulhack");
        assert_eq!(metadata.duration_seconds, 405);
        assert_eq!(
            metadata.playlist_items.as_ref().map(Vec::len),
            Some(2),
            "playlist items should be populated"
        );
        assert_eq!(metadata.available_audio_formats, vec!["mp3"]);
        assert_eq!(
            metadata.thumbnail_url,
            "https://i1.sndcdn.com/artworks-12345-t500x500.jpg"
        );
    }

    #[test]
    fn test_parse_soundcloud_metadata_for_artist_collection() {
        let extract_links = serde_json::json!({
            "kind": "artist",
            "title": "Forss",
            "artwork_url": "https://i1.sndcdn.com/avatars-42.jpg",
            "tracks": [
                {
                    "id": "123",
                    "title": "Flickermood",
                    "url": "https://soundcloud.com/forss/flickermood",
                    "artist": "Forss",
                    "duration_ms": 225000,
                    "artwork_url": null
                },
                {
                    "id": "124",
                    "title": "Journeyman",
                    "url": "https://soundcloud.com/forss/journeyman",
                    "artist": "Forss",
                    "duration_ms": 180000,
                    "artwork_url": null
                }
            ]
        });

        let metadata = parse_soundcloud_metadata(&extract_links.to_string())
            .expect("SoundCloud artist metadata should parse");

        assert!(metadata.is_playlist);
        assert_eq!(metadata.title, "Forss");
        assert_eq!(metadata.duration_seconds, 405);
        assert_eq!(
            metadata.thumbnail_url,
            "https://i1.sndcdn.com/avatars-42.jpg"
        );
        assert_eq!(
            metadata.playlist_items.as_ref().map(Vec::len),
            Some(2),
            "artist tracks should be populated"
        );
    }

    #[test]
    fn test_parse_soundcloud_playlist_targets_filters_selection() {
        let extract_links = serde_json::json!({
            "kind": "playlist",
            "tracks": [
                {
                    "id": "123",
                    "title": "Flickermood",
                    "url": "https://soundcloud.com/forss/flickermood",
                    "artist": "Forss",
                    "duration_ms": 225000,
                    "artwork_url": null
                },
                {
                    "id": "124",
                    "title": "Journeyman",
                    "url": "https://soundcloud.com/forss/journeyman",
                    "artist": "Forss",
                    "duration_ms": 180000,
                    "artwork_url": null
                }
            ]
        });

        let tracks =
            parse_soundcloud_playlist_targets(&extract_links.to_string(), &["124".to_string()])
                .expect("playlist targets should parse");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "124");
        assert_eq!(
            soundcloud_track_download_title(&tracks[0]),
            "Forss - Journeyman"
        );
    }

    // ── parse_ytdlp_json tests ────────────────────────────────────────

    use super::parse_ytdlp_json;

    fn make_format(
        vcodec: &str,
        acodec: &str,
        ext: &str,
        height: u64,
        width: u64,
        fps: f64,
        tbr: f64,
    ) -> serde_json::Value {
        serde_json::json!({
            "vcodec": vcodec,
            "acodec": acodec,
            "ext": ext,
            "height": height,
            "width": width,
            "fps": fps,
            "tbr": tbr
        })
    }

    #[test]
    fn test_parse_ytdlp_basic_video_metadata() {
        let json = serde_json::json!({
            "title": "Test Video",
            "thumbnail": "https://example.com/thumb.jpg",
            "duration": 120.0,
            "_type": "video",
            "formats": [
                make_format("vp9", "opus", "webm", 720, 1280, 30.0, 1500.0),
                make_format("avc1", "mp4a", "mp4", 1080, 1920, 30.0, 4000.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        assert_eq!(result.title, "Test Video");
        assert_eq!(result.thumbnail_url, "https://example.com/thumb.jpg");
        assert_eq!(result.duration_seconds, 120);
        assert!(!result.is_playlist);
        assert!(result.playlist_items.is_none());
    }

    #[test]
    fn test_parse_ytdlp_qualities_deduplicated_and_sorted_by_height_desc() {
        let json = serde_json::json!({
            "title": "Multi Quality",
            "thumbnail": "",
            "duration": 60.0,
            "_type": "video",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 360, 640, 30.0, 500.0),
                make_format("avc1", "mp4a", "mp4", 720, 1280, 30.0, 1500.0),
                make_format("vp9", "opus", "webm", 720, 1280, 30.0, 1200.0),
                make_format("avc1", "mp4a", "mp4", 1080, 1920, 30.0, 4000.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        // 1080p, 720p, 360p — deduplicated and sorted highest first
        assert_eq!(result.available_qualities.len(), 3);
        assert_eq!(result.available_qualities[0].quality, "1080p");
        assert_eq!(result.available_qualities[1].quality, "720p");
        assert_eq!(result.available_qualities[2].quality, "360p");
    }

    #[test]
    fn test_parse_ytdlp_audio_only_formats_extracted_separately() {
        let json = serde_json::json!({
            "title": "Audio Test",
            "thumbnail": "",
            "duration": 60.0,
            "_type": "video",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 720, 1280, 30.0, 1500.0),
                make_format("none", "opus", "webm", 0, 0, 0.0, 128.0),
                make_format("none", "mp4a", "m4a", 0, 0, 0.0, 128.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        assert!(result.available_formats.contains(&"mp4".to_string()));
        assert!(result.available_audio_formats.contains(&"webm".to_string()));
        assert!(result.available_audio_formats.contains(&"m4a".to_string()));
        // Audio-only ext (webm) should NOT appear in video formats when it only appears in audio streams
    }

    #[test]
    fn test_parse_ytdlp_playlist_extracts_entries() {
        let json = serde_json::json!({
            "title": "My Playlist",
            "thumbnail": "",
            "duration": 0.0,
            "_type": "playlist",
            "formats": [],
            "subtitles": {},
            "entries": [
                { "id": "abc123", "title": "Video 1", "duration": 90.0 },
                { "id": "def456", "title": "Video 2", "duration": 180.0 },
            ]
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        assert!(result.is_playlist);
        let items = result.playlist_items.expect("playlist items present");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "abc123");
        assert_eq!(items[1].duration_seconds, 180);
    }

    #[test]
    fn test_parse_ytdlp_drops_youtube_heights_below_360p() {
        // 144p and 240p are DASH-only on YouTube and are NOT declared in
        // vortex-mod-youtube's plugin.toml `default_quality` options. If the
        // UI were to offer them, picking either would fail with yt-dlp's
        // "Requested format is not available" error because
        // `resolve_stream_url` only bypasses its pre-merged-HTTPS path for
        // heights >=720. The metadata IPC must filter these out so the UI
        // only surfaces qualities the plugin actually supports.
        let json = serde_json::json!({
            "title": "Low Res Clip",
            "thumbnail": "",
            "duration": 42.0,
            "_type": "video",
            "extractor_key": "Youtube",
            "webpage_url_domain": "youtube.com",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 144, 256, 30.0, 100.0),
                make_format("avc1", "mp4a", "mp4", 240, 426, 30.0, 300.0),
                make_format("avc1", "mp4a", "mp4", 360, 640, 30.0, 500.0),
                make_format("avc1", "mp4a", "mp4", 1080, 1920, 30.0, 4000.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let heights: Vec<u32> = result
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert!(
            !heights.contains(&144),
            "144p must be filtered out: plugin does not support it"
        );
        assert!(
            !heights.contains(&240),
            "240p must be filtered out: plugin does not support it"
        );
        assert!(heights.contains(&360), "360p must remain");
        assert!(heights.contains(&1080), "1080p must remain");
    }

    #[test]
    fn test_parse_ytdlp_drops_youtube_non_standard_heights() {
        // yt-dlp sometimes reports unusual heights (e.g. 1072 on transcoded
        // uploads) that are not in the plugin's supported set. Only the
        // canonical YouTube ladder {360, 480, 720, 1080, 1440, 2160, 4320}
        // should reach the UI for YouTube sources.
        let json = serde_json::json!({
            "title": "Weird Heights",
            "thumbnail": "",
            "duration": 10.0,
            "_type": "video",
            "extractor_key": "Youtube",
            "webpage_url_domain": "www.youtube.com",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 144, 256, 30.0, 100.0),
                make_format("avc1", "mp4a", "mp4", 270, 480, 30.0, 400.0),
                make_format("avc1", "mp4a", "mp4", 1072, 1920, 30.0, 3900.0),
                make_format("avc1", "mp4a", "mp4", 1080, 1920, 30.0, 4000.0),
                make_format("avc1", "mp4a", "mp4", 2160, 3840, 60.0, 20000.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let heights: Vec<u32> = result
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert_eq!(
            heights,
            vec![2160, 1080],
            "only canonical ladder heights must survive"
        );
    }

    #[test]
    fn test_parse_ytdlp_preserves_vimeo_non_canonical_heights() {
        // Regression guard for PR #79 review: `parse_ytdlp_json` is shared
        // across all extractors, so the YouTube height allow-list must NOT
        // apply to Vimeo. Vimeo serves 540p (and other off-ladder sizes like
        // 640p), which must reach the quality selector for those sources.
        let json = serde_json::json!({
            "title": "Vimeo Sample",
            "thumbnail": "",
            "duration": 60.0,
            "_type": "video",
            "extractor_key": "Vimeo",
            "webpage_url_domain": "vimeo.com",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 240, 426, 30.0, 300.0),
                make_format("avc1", "mp4a", "mp4", 360, 640, 30.0, 700.0),
                make_format("avc1", "mp4a", "mp4", 540, 960, 30.0, 1500.0),
                make_format("avc1", "mp4a", "mp4", 720, 1280, 30.0, 2500.0),
                make_format("avc1", "mp4a", "mp4", 1080, 1920, 30.0, 4000.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let heights: Vec<u32> = result
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert_eq!(
            heights,
            vec![1080, 720, 540, 360, 240],
            "Vimeo heights must not be filtered through the YouTube allow-list"
        );
    }

    #[test]
    fn test_parse_ytdlp_detects_youtube_via_webpage_domain_only() {
        // Belt-and-braces: if yt-dlp renames `extractor_key` in a future
        // release but still sets `webpage_url_domain`, YouTube filtering must
        // still fire for known YouTube endpoints (youtu.be, music.youtube.com
        // …).
        let json = serde_json::json!({
            "title": "Shared Short",
            "thumbnail": "",
            "duration": 15.0,
            "_type": "video",
            "webpage_url_domain": "youtu.be",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 240, 426, 30.0, 300.0),
                make_format("avc1", "mp4a", "mp4", 720, 1280, 30.0, 2500.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let heights: Vec<u32> = result
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert_eq!(heights, vec![720], "240p must be dropped on youtu.be URLs");
    }

    #[test]
    fn test_parse_ytdlp_unknown_source_keeps_all_positive_heights() {
        // Payloads without an extractor key (e.g. generic extractor, unknown
        // fallback) must not be assumed to be YouTube: keep every positive
        // height so new providers work without modifying this parser.
        let json = serde_json::json!({
            "title": "Unknown Source",
            "thumbnail": "",
            "duration": 30.0,
            "_type": "video",
            "formats": [
                make_format("avc1", "mp4a", "mp4", 144, 256, 30.0, 100.0),
                make_format("avc1", "mp4a", "mp4", 480, 854, 30.0, 800.0),
                make_format("avc1", "mp4a", "mp4", 540, 960, 30.0, 1500.0),
            ],
            "subtitles": {}
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let heights: Vec<u32> = result
            .available_qualities
            .iter()
            .map(|q| q.height)
            .collect();
        assert_eq!(
            heights,
            vec![540, 480, 144],
            "unknown-source heights must survive, including 144p and 540p"
        );
    }

    #[test]
    fn test_parse_ytdlp_subtitles_excludes_live_chat() {
        let json = serde_json::json!({
            "title": "Live Video",
            "thumbnail": "",
            "duration": 0.0,
            "_type": "video",
            "formats": [],
            "subtitles": {
                "en": [{"ext": "vtt"}],
                "fr": [{"ext": "vtt"}],
                "live_chat": [{"ext": "json"}]
            }
        });

        let result = parse_ytdlp_json(&json).expect("parse should succeed");

        let codes: Vec<&str> = result
            .available_subtitles
            .iter()
            .map(|s| s.code.as_str())
            .collect();
        assert!(codes.contains(&"en"));
        assert!(codes.contains(&"fr"));
        assert!(!codes.contains(&"live_chat"));
    }
}
