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
    find_approved_named_binary(
        candidates,
        roots,
        if cfg!(windows) {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        },
    )
    .context(INSTALL_REMEDIATION)
}

#[cfg(unix)]
const INSTALL_REMEDIATION: &str = "yt-dlp not found in approved locations; install or update it at ~/.local/bin/yt-dlp or with a supported system package";

#[cfg(windows)]
const INSTALL_REMEDIATION: &str = "yt-dlp not found in approved locations; install it with WinGet or at %LOCALAPPDATA%\\Programs\\yt-dlp\\yt-dlp.exe";

pub(crate) fn find_approved_named_binary(
    candidates: &[PathBuf],
    roots: &[PathBuf],
    expected_name: &str,
) -> anyhow::Result<PathBuf> {
    for candidate in candidates {
        let Ok(canonical) = std::fs::canonicalize(candidate) else {
            continue;
        };
        if valid_binary(&canonical, roots, expected_name)? {
            return Ok(canonical);
        }
    }
    bail!("approved executable '{expected_name}' was not found")
}

fn valid_binary(path: &Path, roots: &[PathBuf], expected_name: &str) -> anyhow::Result<bool> {
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
        return Ok(false);
    }
    let metadata = std::fs::metadata(path)?;
    let Some(root) = approved_root_for(path, roots) else {
        return Ok(false);
    };
    if !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0
            || mode & 0o022 != 0
            || !trusted_owner(metadata.uid())
            || !trusted_ancestor_chain(path, &root)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn approved_root_for(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
}

#[cfg(unix)]
fn trusted_ancestor_chain(path: &Path, root: &Path) -> anyhow::Result<bool> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let Some(mut current) = path.parent() else {
        return Ok(false);
    };
    loop {
        let metadata = std::fs::metadata(current)?;
        if !metadata.is_dir()
            || !trusted_owner(metadata.uid())
            || metadata.permissions().mode() & 0o022 != 0
        {
            return Ok(false);
        }
        if current == root {
            return Ok(true);
        }
        let Some(parent) = current.parent() else {
            return Ok(false);
        };
        current = parent;
    }
}

#[cfg(unix)]
fn trusted_owner(uid: u32) -> bool {
    uid == 0 || uid == unsafe { libc::geteuid() }
}

pub(super) fn controlled_path(binary: &Path) -> anyhow::Result<OsString> {
    controlled_path_with_roots(binary, &approved_roots())
}

pub(super) fn controlled_path_with_roots(
    binary: &Path,
    roots: &[PathBuf],
) -> anyhow::Result<OsString> {
    let mut paths = Vec::new();
    if let Some(parent) = binary.parent()
        && let Some(parent) = trusted_directory(parent, roots)
    {
        paths.push(parent);
    }
    #[cfg(unix)]
    paths.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/run/current-system/sw/bin",
            "/nix/var/nix/profiles/default/bin",
        ]
        .into_iter()
        .filter_map(|path| trusted_directory(Path::new(path), roots)),
    );
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
        paths.extend(
            [root.join("System32"), root]
                .into_iter()
                .filter_map(|path| trusted_directory(&path, roots)),
        );
    }
    paths.sort();
    paths.dedup();
    std::env::join_paths(paths).context("run_ytdlp: approved PATH cannot be encoded")
}

fn trusted_directory(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&canonical).ok()?;
    if !metadata.is_dir() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = approved_root_for(&canonical, roots)?;
        if !trusted_owner(metadata.uid())
            || metadata.permissions().mode() & 0o022 != 0
            || !trusted_ancestor_chain(&canonical.join(".vortex-helper-check"), &root).ok()?
        {
            return None;
        }
    }
    #[cfg(not(unix))]
    let _ = roots;
    Some(canonical)
}

#[cfg(windows)]
pub(crate) fn copy_required_environment(command: &mut Command) {
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn copy_required_environment(_command: &mut Command) {}
