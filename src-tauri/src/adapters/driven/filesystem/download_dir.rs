//! System Downloads directory resolver.
//!
//! Wraps `dirs::download_dir()` so the domain stays free of infra concerns.
//! Returned as `String` because `AppConfig::download_dir` is `Option<String>`.

/// Returns the OS-specific default Downloads directory, if one exists.
///
/// Platforms:
/// - Linux : `$XDG_DOWNLOAD_DIR` or `~/Downloads`
/// - macOS : `~/Downloads`
/// - Windows : `%USERPROFILE%\Downloads`
/// - Others / FHS-only : `None`
pub fn resolve_system_download_dir() -> Option<String> {
    dirs::download_dir().and_then(|p| p.to_str().map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn resolves_some_path_on_supported_platforms() {
        let result = resolve_system_download_dir();
        assert!(
            result.is_some(),
            "dirs::download_dir() should return a path on Linux/macOS/Windows"
        );
        let path = result.unwrap();
        assert!(!path.is_empty(), "resolved path must not be empty");
    }

    #[test]
    fn returns_string_not_pathbuf_for_config_compatibility() {
        // Contract test: AppConfig::download_dir is Option<String>,
        // so this function MUST also return Option<String>.
        let _assert_type: Option<String> = resolve_system_download_dir();
    }
}
