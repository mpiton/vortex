//! Handler for `StartDownloadCommand`.
//!
//! Validates the URL, probes the remote server for metadata,
//! creates the `Download` aggregate, persists it, and emits
//! `DownloadCreated` so the queue manager can schedule it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::download_source_policy::classify_download_module;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{AccountId, AccountStatus};
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::http::HttpResponse;

/// Monotonic counter combined with nanosecond timestamp for restart-safe
/// ID generation. The counter prevents collisions within a process; the
/// timestamp prevents collisions across restarts.
static NEXT_DOWNLOAD_SEQ: AtomicU64 = AtomicU64::new(0);

impl CommandBus {
    pub async fn handle_start_download(
        &self,
        cmd: super::StartDownloadCommand,
    ) -> Result<DownloadId, AppError> {
        let url = Url::new(&cmd.url)?;

        // Use the pre-computed filename when available (e.g. set by media plugins
        // that already know the video title). Otherwise probe via HEAD or fall back
        // to extracting the last URL path segment.
        //
        // Reject path-bearing overrides: since `dest_dir.join(&file_name)` is
        // used below, an absolute path or one containing `..` would escape the
        // configured download directory.
        let file_name = if let Some(name) = cmd.filename.as_deref().filter(|s| !s.is_empty()) {
            let candidate = std::path::Path::new(name);
            if candidate.is_absolute() || candidate.components().count() != 1 {
                return Err(AppError::Domain(
                    crate::domain::error::DomainError::ValidationError(format!(
                        "invalid filename override (must be a single file component): {name}"
                    )),
                ));
            }
            name.to_string()
        } else if classify_download_module(self.plugin_loader(), cmd.module_name.as_deref())?
            .is_protected()
        {
            // Hoster URLs may be plugin-generated stable identifiers. Never send
            // them through the unrestricted generic HTTP adapter before JIT
            // resolution applies its restricted-network policy.
            filename_from_url(&url)
        } else {
            // file_size and resume_supported are discovered by the engine at download time.
            // The HEAD probe here is used only for filename resolution.
            match self.http_client().head(url.as_str()) {
                Ok(resp) => extract_filename(&resp, &url),
                Err(_) => filename_from_url(&url),
            }
        };

        let dest_dir = cmd.destination.unwrap_or_else(|| {
            // Prefer user-configured download dir; fall back to ~/Downloads/
            self.config_store()
                .get_config()
                .ok()
                .and_then(|c| c.download_dir)
                .map(PathBuf::from)
                .or_else(dirs::download_dir)
                .unwrap_or_else(|| PathBuf::from("."))
        });
        let dest = dest_dir.join(&file_name);

        let id = next_download_id();
        let account_lock = cmd
            .account_id
            .as_ref()
            .map(|id| self.account_operation_lock(id))
            .transpose()?;
        let _account_guard = match account_lock {
            Some(lock) => Some(lock.lock_owned().await),
            None => None,
        };
        self.validate_download_account(cmd.module_name.as_deref(), cmd.account_id.as_ref())?;
        // Append to the back of the queue so a freshly added download
        // does not jump in front of items the user has explicitly
        // reordered (default queue_position 0 would sort before 1..N).
        // Hold the queue-position lock so the read+write is atomic vs.
        // concurrent move_to_top/move_to_bottom/start_download calls.
        let _guard = self.lock_queue_positions().await;
        let queue_position = super::move_queue::next_queue_position(self.download_repo())?;

        let mut download = Download::new(id, url, file_name, dest.to_string_lossy().to_string())
            .with_queue_position(queue_position)
            .with_remote_metadata(cmd.size_bytes, cmd.resume_supported);

        if let Some(hostname) = cmd.source_hostname_override {
            download = download.with_source_hostname(hostname);
        }
        if let Some(module_name) = cmd.module_name {
            download = download.with_module_name(module_name);
        }
        if let Some(account_id) = cmd.account_id {
            download = download.with_account_id(account_id);
        }

        self.download_repo().save(&download)?;
        self.event_bus()
            .publish(DomainEvent::DownloadCreated { id });

        Ok(id)
    }

    pub(super) fn validate_download_account(
        &self,
        module_name: Option<&str>,
        account_id: Option<&AccountId>,
    ) -> Result<(), AppError> {
        let (Some(module_name), Some(account_id)) = (module_name, account_id) else {
            if account_id.is_some() {
                return Err(AppError::Validation(
                    "account association requires a plugin name".into(),
                ));
            }
            return Ok(());
        };
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;
        let mut account = repo.find_by_id(account_id)?.ok_or_else(|| {
            AppError::NotFound(format!("account {} not found", account_id.as_str()))
        })?;
        if account.service_name() != module_name {
            return Err(AppError::Validation(format!(
                "account {} is not compatible with plugin {module_name}",
                account_id.as_str()
            )));
        }
        if store.get_password(account_id)?.is_none() {
            account.set_status(AccountStatus::MissingCredential);
            self.save_account_availability(repo, &account)?;
            self.event_bus()
                .publish(DomainEvent::AccountValidationFailed {
                    id: account_id.clone(),
                    error: "Account credential is unavailable".into(),
                });
            return Err(AppError::NotFound(format!(
                "credential for account {} not found",
                account_id.as_str()
            )));
        }
        if !account.is_selectable(self.account_now_ms()?) {
            return Err(AppError::Validation(format!(
                "account {} is not available",
                account_id.as_str()
            )));
        }
        Ok(())
    }
}

/// Generate a restart-safe, collision-resistant download ID that fits
/// within JavaScript's `Number.MAX_SAFE_INTEGER` (2^53).
///
/// Layout: millisecond timestamp in high 41 bits, monotonic counter in
/// low 12 bits. Disjoint bit ranges prevent the `(T, seq)` vs
/// `(T+seq, 0)` collision class. 12-bit counter allows 4096 downloads
/// per millisecond.
pub(super) fn next_download_id() -> DownloadId {
    let seq = NEXT_DOWNLOAD_SEQ.fetch_add(1, Ordering::Relaxed) & 0xFFF;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    DownloadId((ts << 12) | seq)
}

fn extract_filename(resp: &HttpResponse, url: &Url) -> String {
    if let Some(cd) = resp.header("content-disposition")
        && let Some(name) = parse_content_disposition(cd)
    {
        return name;
    }
    filename_from_url(url)
}

fn filename_from_url(url: &Url) -> String {
    url.as_str()
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("download")
        .to_string()
}

fn parse_content_disposition(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let part = part.trim();
        if part.starts_with("filename=") {
            Some(
                part.trim_start_matches("filename=")
                    .trim_matches('"')
                    .to_string(),
            )
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "start_download_tests.rs"]
mod tests;
