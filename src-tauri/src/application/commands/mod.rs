//! CQRS command types and handlers.
//!
//! Each command represents an intent to mutate application state.
//! Handler implementations live in submodules and add methods to `CommandBus`.

mod cancel_download;
mod install_plugin;
mod pause_all;
mod pause_download;
mod remove_download;
mod resolve_links;
mod resume_all;
mod resume_download;
mod retry_download;
mod set_priority;
mod start_download;
mod toggle_clipboard;
mod toggle_plugin;
mod uninstall_plugin;
mod update_config;

use std::path::PathBuf;

use crate::domain::model::config::ConfigPatch;
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driving::Command;

#[derive(Debug)]
pub struct StartDownloadCommand {
    pub url: String,
    pub destination: Option<PathBuf>,
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
