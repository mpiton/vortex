//! CQRS command types.
//!
//! Each command represents an intent to mutate application state.
//! Handlers will be implemented in later tasks.
#![allow(dead_code)] // All commands consumed by handlers (tasks 11-12)

use crate::domain::model::config::ConfigPatch;
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driving::Command;

#[derive(Debug)]
pub struct StartDownloadCommand {
    pub url: String,
    pub destination: Option<String>,
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

#[derive(Debug)]
pub struct InstallPluginCommand {
    pub url: String,
}
impl Command for InstallPluginCommand {}

#[derive(Debug)]
pub struct UninstallPluginCommand {
    pub name: String,
}
impl Command for UninstallPluginCommand {}

#[derive(Debug)]
pub struct UpdateConfigCommand {
    pub patch: ConfigPatch,
}
impl Command for UpdateConfigCommand {}
