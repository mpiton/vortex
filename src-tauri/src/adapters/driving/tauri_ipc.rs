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
#[tauri::command]
pub async fn download_media_start(
    state: State<'_, AppState>,
    url: String,
    quality: String,
    format: String,
    audio_only: bool,
    title: Option<String>,
) -> Result<u64, String> {
    // Validate the format extension before anything uses it in a filename —
    // `format!("{}.{}", sanitize_filename(title), format)` below would otherwise
    // interpolate attacker-controlled path separators after the last `/` char
    // that `sanitize_filename` already escaped in the stem.
    let format = sanitize_extension(&format)?;

    // Extract the origin hostname (e.g. "www.youtube.com") from the original
    // URL *before* resolving to a CDN URL, so we can store it as the
    // source_hostname instead of "rr1---sn-n4g-cvq6.googlevideo.com".
    let source_hostname_override = extract_hostname_from_url(&url);

    let plugin_loader = state.plugin_loader.clone();
    let url_clone = url.clone();
    let quality_clone = quality.clone();
    let format_clone = format.clone();
    let title_clone = title.clone();

    // Plugin calls are synchronous (Extism runs inside a Mutex). Run on the
    // blocking thread pool so we don't starve the async executor.
    enum StreamResolution {
        CdnUrl(String),
        LocalFile {
            path: std::path::PathBuf,
            size: u64,
            filename: String,
        },
    }

    let resolution = tokio::task::spawn_blocking(move || -> Result<StreamResolution, String> {
        match plugin_loader.resolve_stream_url(
            &url_clone,
            &quality_clone,
            &format_clone,
            audio_only,
        ) {
            Ok(cdn_url) => Ok(StreamResolution::CdnUrl(cdn_url)),

            Err(crate::domain::error::DomainError::AdaptiveStreamOnly) => {
                // yt-dlp must handle the full download+merge.
                let temp_dir = std::env::temp_dir().join("vortex-downloads");
                std::fs::create_dir_all(&temp_dir)
                    .map_err(|e| format!("failed to create temp dir: {e}"))?;

                let file_info = plugin_loader
                    .download_to_file(
                        &url_clone,
                        &quality_clone,
                        &format_clone,
                        temp_dir.to_str()
                            .ok_or_else(|| "temp dir path is not valid UTF-8".to_string())?,
                        audio_only,
                    )
                    .map_err(|e| format!("download_to_file failed: {e}"))?;

                // Defense in depth: `ExtismPluginLoader::download_to_file` already
                // enforces that the returned path is inside `output_dir`, but the
                // `PluginLoader` trait does not require it — a future or alternate
                // loader implementation could return an arbitrary path and we'd
                // happily move it. Re-check containment here at the IPC boundary.
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

                // Determine final filename: prefer title override, else keep yt-dlp's name.
                let filename = title_clone
                    .as_deref()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| format!("{}.{}", sanitize_filename(t), format_clone))
                    .unwrap_or_else(|| {
                        file_info
                            .path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("download")
                            .to_string()
                    });

                // Determine final destination directory. Prefer the platform
                // download dir (XDG `user-dirs`, `~/Downloads`, …); fall back to
                // the home dir rather than CWD — dropping a 1GB merged video
                // into the Tauri binary's directory or / is a bad outcome.
                let dest_dir = dirs::download_dir()
                    .or_else(dirs::home_dir)
                    .ok_or_else(|| {
                        "cannot determine download destination: neither \
                         user-dirs download_dir nor home_dir are available"
                            .to_string()
                    })?;
                // The user-dirs `Downloads` folder can be configured to a path
                // that hasn't been created yet (e.g. a fresh user account, or a
                // CI environment). Ensure it exists before probing filenames.
                std::fs::create_dir_all(&dest_dir)
                    .map_err(|e| format!("failed to create destination dir {}: {e}",
                        dest_dir.display()))?;
                // If the destination already exists, suffix the filename (" (1)",
                // " (2)", …) — preserves the previous download instead of silently
                // overwriting it, matching the browser-download convention.
                let (dest_path, dest_filename) = unique_destination(&dest_dir, &filename)
                    .map_err(|e| format!("failed to select unique destination: {e}"))?;

                // Atomic move (same filesystem) → fallback copy+delete.
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

            // builtin-http: no WASM plugin claimed the URL.
            // For known media platforms this means the required plugin is not
            // installed — return a clear error rather than feeding the HTML
            // page URL to the download engine and entering a retry loop.
            Err(crate::domain::error::DomainError::NotFound(_)) => {
                if is_known_media_platform(&url_clone) {
                    Err(
                        "No media plugin installed for this URL. \
                         Open the Plugin Store and install the appropriate plugin (e.g. vortex-mod-youtube)."
                            .to_string(),
                    )
                } else {
                    // Generic direct-download URL — pass through as-is.
                    Ok(StreamResolution::CdnUrl(url_clone))
                }
            }

            Err(e) => Err(format!("Failed to resolve stream URL: {e}")),
        }
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    match resolution {
        StreamResolution::CdnUrl(stream_url) => {
            // Build a meaningful filename from the title and format when available.
            // Falls back to the URL-based derivation in handle_start_download when None.
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
            state
                .command_bus
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
            state
                .command_bus
                .handle_register_local_file(cmd)
                .await
                .map(|id| id.0)
                .map_err(|e| e.to_string())
        }
    }
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

// ── Media Metadata ───────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadataDto {
    pub title: String,
    pub thumbnail_url: String,
    pub duration_seconds: u64,
    pub is_playlist: bool,
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
pub async fn command_get_media_metadata(url: String) -> Result<MediaMetadataDto, String> {
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
        // Build quality list: deduplicated by height, sorted highest first
        let mut video_formats: Vec<&serde_json::Value> = formats
            .iter()
            .filter(|f| f["vcodec"].as_str().unwrap_or("none") != "none")
            .filter(|f| f["height"].as_u64().unwrap_or(0) > 0)
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
        thumbnail_url,
        duration_seconds,
        is_playlist,
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
        configured_status_bar_path, extract_hostname_from_url, read_available_space,
        resolve_existing_disk_path, sanitize_extension, sanitize_filename, unique_destination,
    };
    use std::path::PathBuf;

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
