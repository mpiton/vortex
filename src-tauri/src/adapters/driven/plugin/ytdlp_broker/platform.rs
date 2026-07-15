use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

pub(super) fn discover_ytdlp() -> anyhow::Result<PathBuf> {
    find_approved_binary(&candidates(), &approved_roots())
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/bin/yt-dlp"));
        paths.push(home.join(".nix-profile/bin/yt-dlp"));
    }
    #[cfg(unix)]
    paths.extend([
        PathBuf::from("/opt/homebrew/bin/yt-dlp"),
        PathBuf::from("/usr/local/bin/yt-dlp"),
        PathBuf::from("/usr/bin/yt-dlp"),
        PathBuf::from("/run/current-system/sw/bin/yt-dlp"),
        PathBuf::from("/nix/var/nix/profiles/default/bin/yt-dlp"),
    ]);
    #[cfg(windows)]
    add_windows_candidates(&mut paths);
    paths
}

#[cfg(windows)]
fn add_windows_candidates(paths: &mut Vec<PathBuf>) {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return;
    };
    paths.push(local.join("Programs/yt-dlp/yt-dlp.exe"));
    let packages = local.join("Microsoft/WinGet/Packages");
    let Ok(entries) = std::fs::read_dir(packages) else {
        return;
    };
    paths.extend(entries.filter_map(Result::ok).filter_map(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("yt-dlp.yt-dlp_")
            .then(|| entry.path().join("yt-dlp.exe"))
    }));
}

fn approved_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    #[cfg(unix)]
    roots.extend([
        PathBuf::from("/opt/homebrew"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/usr"),
        PathBuf::from("/run/current-system"),
        PathBuf::from("/nix/store"),
        PathBuf::from("/nix/var/nix/profiles"),
    ]);
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local));
    }
    roots
}

pub(super) fn find_approved_binary(
    candidates: &[PathBuf],
    roots: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    for candidate in candidates {
        let Ok(canonical) = std::fs::canonicalize(candidate) else {
            continue;
        };
        if valid_binary(&canonical, roots)? {
            return Ok(canonical);
        }
    }
    bail!("yt-dlp not found in approved locations; install it with: pip install yt-dlp")
}

fn valid_binary(path: &Path, roots: &[PathBuf]) -> anyhow::Result<bool> {
    let expected = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    if path.file_name().and_then(|name| name.to_str()) != Some(expected) {
        return Ok(false);
    }
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || !inside_approved_root(path, roots) {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 || mode & 0o022 != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn inside_approved_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        path.starts_with(canonical)
    })
}

pub(super) fn controlled_path(binary: &Path) -> anyhow::Result<OsString> {
    let mut paths = Vec::new();
    if let Some(parent) = binary.parent() {
        paths.push(parent.to_path_buf());
    }
    paths.extend(
        candidates()
            .into_iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf)),
    );
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
        paths.extend([root.join("System32"), root]);
    }
    paths.sort();
    paths.dedup();
    std::env::join_paths(paths).context("run_ytdlp: approved PATH cannot be encoded")
}

#[cfg(windows)]
pub(super) fn copy_required_environment(command: &mut Command) {
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

#[cfg(not(windows))]
pub(super) fn copy_required_environment(_command: &mut Command) {}
