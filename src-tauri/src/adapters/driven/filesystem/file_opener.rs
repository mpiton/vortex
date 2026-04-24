//! Platform-backed [`FileOpener`] implementation.
//!
//! Uses the native launcher for each OS. Linux falls back to `xdg-open`
//! without file selection because no portable spec equivalent exists.
//!
//! These calls spawn a child process and wait for it to exit. Callers that
//! must not block a hot path should schedule this on a blocking tokio pool.

use std::path::Path;
use std::process::Command;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::FileOpener;

pub struct SystemFileOpener;

impl Default for SystemFileOpener {
    fn default() -> Self {
        Self
    }
}

impl SystemFileOpener {
    pub fn new() -> Self {
        Self
    }
}

impl FileOpener for SystemFileOpener {
    fn open_file(&self, path: &Path) -> Result<(), DomainError> {
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "file not found: {}",
                path.display()
            )));
        }
        if !path.is_file() {
            return Err(DomainError::ValidationError(format!(
                "not a regular file: {}",
                path.display()
            )));
        }
        #[cfg(target_os = "linux")]
        let (program, args): (&str, Vec<std::ffi::OsString>) =
            ("xdg-open", vec![path.as_os_str().to_os_string()]);
        #[cfg(target_os = "macos")]
        let (program, args): (&str, Vec<std::ffi::OsString>) =
            ("open", vec![path.as_os_str().to_os_string()]);
        #[cfg(target_os = "windows")]
        let (program, args): (&str, Vec<std::ffi::OsString>) = (
            "cmd",
            vec![
                std::ffi::OsString::from("/C"),
                std::ffi::OsString::from("start"),
                std::ffi::OsString::from(""),
                path.as_os_str().to_os_string(),
            ],
        );

        run_launcher(program, &args)
    }

    fn reveal_file(&self, path: &Path) -> Result<(), DomainError> {
        // Parent resolution must succeed even when the file itself is gone —
        // that way the UI can still jump to the containing directory after a
        // manual move/delete. We only error when both the file and its parent
        // are missing (path is a bare filename with no anchor on disk).
        //
        // `Path::parent` returns `Some("")` for single-component relative paths
        // (e.g. `Path::new("file.bin").parent() == Some("")`). Treat that
        // empty path as the current working directory so we don't mistakenly
        // declare the folder missing and don't hand an empty arg to xdg-open.
        let parent = match path.parent() {
            Some(p) if p.as_os_str().is_empty() => Some(Path::new(".")),
            Some(p) if p.is_dir() => Some(p),
            _ => None,
        };
        if !path.exists() && parent.is_none() {
            return Err(DomainError::NotFound(format!(
                "file and parent folder both missing: {}",
                path.display()
            )));
        }

        #[cfg(target_os = "linux")]
        {
            // xdg-open does not support selecting a file, so we hand it the
            // parent directory. When the file is gone too we fall back to the
            // parent (already guarded above).
            let target = parent.unwrap_or_else(|| Path::new("."));
            run_launcher("xdg-open", &[target.as_os_str().to_os_string()])
        }

        #[cfg(target_os = "macos")]
        {
            // `open -R` reveals + selects the file in Finder, or falls back
            // to the folder if the file was removed.
            if path.exists() {
                run_launcher(
                    "open",
                    &[
                        std::ffi::OsString::from("-R"),
                        path.as_os_str().to_os_string(),
                    ],
                )
            } else if let Some(dir) = parent {
                run_launcher("open", &[dir.as_os_str().to_os_string()])
            } else {
                Err(DomainError::NotFound(format!(
                    "file and parent folder both missing: {}",
                    path.display()
                )))
            }
        }

        #[cfg(target_os = "windows")]
        {
            // explorer.exe returns non-zero even on success, so we do not
            // check the exit code here. The behaviour mirrors `reveal_in_folder`.
            //
            // Pass "/select," and the path as two separate OsStrings so the
            // Rust Command quoting rules can protect the path boundary when
            // the path contains spaces (e.g. `C:\My Downloads\file.mp4`).
            // Embedding the path in `format!("/select,{path}")` would leave
            // Explorer's own parser stripping everything after the first space.
            let args: Vec<std::ffi::OsString> = if path.exists() {
                vec![
                    std::ffi::OsString::from("/select,"),
                    path.as_os_str().to_os_string(),
                ]
            } else {
                vec![
                    parent
                        .map(|p| p.as_os_str().to_os_string())
                        .unwrap_or_else(|| path.as_os_str().to_os_string()),
                ]
            };
            let status = Command::new("explorer").args(&args).status().map_err(|e| {
                DomainError::StorageError(format!("failed to launch explorer: {e}"))
            })?;
            let _ = status;
            Ok(())
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run_launcher(program: &str, args: &[std::ffi::OsString]) -> Result<(), DomainError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| DomainError::StorageError(format!("failed to launch {program}: {e}")))?;
    if !status.success() {
        return Err(DomainError::StorageError(format!(
            "{program} exited with status {status}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_launcher(program: &str, args: &[std::ffi::OsString]) -> Result<(), DomainError> {
    let _status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| DomainError::StorageError(format!("failed to launch {program}: {e}")))?;
    // Launchers on Windows (cmd/start/explorer) can return non-zero even on
    // successful hand-off, so we trust the spawn-did-not-error signal only.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_errors_when_path_missing() {
        let opener = SystemFileOpener::new();
        let err = opener
            .open_file(Path::new("/does/not/exist/vortex-test.bin"))
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn open_file_errors_when_path_is_directory() {
        let dir = std::env::temp_dir();
        let opener = SystemFileOpener::new();
        let err = opener.open_file(&dir).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)), "{err:?}");
    }

    #[test]
    fn reveal_file_errors_when_file_and_parent_missing() {
        let opener = SystemFileOpener::new();
        let err = opener
            .reveal_file(Path::new("/does/not/exist-either/sub/vortex-test.bin"))
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
    }
}
