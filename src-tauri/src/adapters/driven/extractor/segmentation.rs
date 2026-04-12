//! Split archive detection module.
//!
//! Detects and validates multi-part archive segments (RAR, 7z, ZIP) in the filesystem.
//! Supports various archive formats:
//! - RAR: `name.part01.rar`, `name.part02.rar`, ... or `name.rar`, `name.r00`, `name.r01`
//! - 7z: `name.7z.001`, `name.7z.002`, ...
//! - ZIP: `name.zip.001`, `name.zip.002`, ... or `name.z01`, `name.z02`, ...

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::domain::error::DomainError;

/// Detects if a file is part of a split archive set.
///
/// # Arguments
/// * `file_path` - Path to the archive file to check
///
/// # Returns
/// * `Ok(Some(parts))` - If the file is part of a multi-part archive, returns sorted list of all parts
/// * `Ok(None)` - If the file is a single archive (not split)
/// * `Err(DomainError::StorageError)` - If I/O error occurs
///
/// # Examples
/// ```ignore
/// let parts = detect_segments(Path::new("/downloads/archive.part01.rar"))?;
/// // Returns Some(["/downloads/archive.part01.rar", "/downloads/archive.part02.rar", ...])
/// ```
pub fn detect_segments(file_path: &Path) -> Result<Option<Vec<PathBuf>>, DomainError> {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| DomainError::StorageError("Invalid file path".to_string()))?;

    // Try modern RAR format: name.part01.rar, name.part02.rar
    if let Some(parts) = detect_rar_modern(file_path, file_name)? {
        return Ok(Some(parts));
    }

    // Try legacy RAR format: name.rar, name.r00, name.r01
    if let Some(parts) = detect_rar_legacy(file_path, file_name)? {
        return Ok(Some(parts));
    }

    // Try 7z format: name.7z.001, name.7z.002
    if let Some(parts) = detect_7z(file_path, file_name)? {
        return Ok(Some(parts));
    }

    // Try ZIP format: name.zip.001, name.zip.002 or name.z01, name.z02
    if let Some(parts) = detect_zip(file_path, file_name)? {
        return Ok(Some(parts));
    }

    Ok(None)
}

/// Verifies that all parts in a segment list exist and have no gaps in numbering.
///
/// # Arguments
/// * `parts` - List of part paths to verify
///
/// # Returns
/// * `Ok(parts)` - If all parts exist and numbering is continuous
/// * `Err(DomainError::StorageError)` - If any part is missing or numbering has gaps
///
/// # Examples
/// ```ignore
/// let parts = vec![
///     PathBuf::from("/downloads/archive.part01.rar"),
///     PathBuf::from("/downloads/archive.part02.rar"),
/// ];
/// verify_all_parts_present(&parts)?;
/// ```
pub fn verify_all_parts_present(parts: &[PathBuf]) -> Result<Vec<PathBuf>, DomainError> {
    if parts.is_empty() {
        return Err(DomainError::StorageError("No parts provided".to_string()));
    }

    let mut missing = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if !part.exists() {
            missing.push(part.clone());
        }

        if i > 0 {
            let curr_name = part.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let curr_ext = Path::new(curr_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let curr_num = extract_part_number(curr_name);

            // Skip continuity check for terminal segments without a numeric part
            // (e.g., "archive.rar" in legacy sets, "archive.zip" in split ZIP sets)
            // But NOT for "archive.part01.rar" which has a numeric part.
            let is_unnumbered_terminal = curr_num.is_none()
                && (curr_ext.eq_ignore_ascii_case("rar") || curr_ext.eq_ignore_ascii_case("zip"));
            if is_unnumbered_terminal {
                continue;
            }

            let prev_num = extract_part_number(parts[i - 1].file_name().unwrap().to_str().unwrap())
                .unwrap_or(0);
            let curr_num = curr_num.unwrap_or(0);

            if curr_num != prev_num + 1 {
                missing.push(
                    part.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(format!("{:02}", prev_num + 1)),
                );
            }
        }
    }

    // Check if the first numeric part starts at the expected baseline.
    // Determine baseline: 0 for 0-indexed sets (r00, r01, ...), 1 otherwise.
    let has_zero_part = parts.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(extract_part_number)
            == Some(0)
    });
    let baseline: u32 = if has_zero_part { 0 } else { 1 };

    // Find first numbered segment, skipping terminal .rar/.zip entries.
    let first_numeric = parts.iter().find_map(|p| {
        let name = p.file_name()?.to_str()?;
        let ext = Path::new(name).extension()?.to_str()?;
        // Skip terminal .rar/.zip unless the stem has a part-numbering pattern
        if ext.eq_ignore_ascii_case("rar") || ext.eq_ignore_ascii_case("zip") {
            let stem = Path::new(name).file_stem()?.to_str()?;
            let stem_ext = Path::new(stem).extension()?.to_str()?;
            if !is_part_pattern(stem_ext) {
                return None;
            }
        }
        let num = extract_part_number(name)?;
        Some((p, num))
    });

    if let Some((part, first_num)) = first_numeric
        && first_num > baseline
        && let Some(parent) = part.parent()
    {
        missing.push(parent.join(format!("{:02}", first_num - 1)));
    }

    if !missing.is_empty() {
        let missing_names: Vec<String> = missing
            .iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str().map(|s| s.to_string()))
            })
            .collect();
        return Err(DomainError::StorageError(format!(
            "Missing archive parts: {}",
            missing_names.join(", ")
        )));
    }

    Ok(parts.to_vec())
}

/// Detect modern RAR multi-part archive segments (.part01.rar, .part02.rar, ...).
fn detect_rar_modern(
    file_path: &Path,
    file_name: &str,
) -> Result<Option<Vec<PathBuf>>, DomainError> {
    let re = Regex::new(r"^(.+)\.part(\d+)\.rar$")
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    if let Some(caps) = re.captures(file_name) {
        let base_name = &caps[1];
        let parent = file_path
            .parent()
            .ok_or_else(|| DomainError::StorageError("No parent directory".to_string()))?;

        return scan_parts(parent, base_name, ".part", ".rar");
    }

    Ok(None)
}

/// Detect legacy RAR multi-part archive segments (.rar, .r00, .r01, ...).
fn detect_rar_legacy(
    file_path: &Path,
    file_name: &str,
) -> Result<Option<Vec<PathBuf>>, DomainError> {
    // Match: name.rar or name.r00, name.r01, etc.
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let is_rar_ext = ext == "rar"
        || (ext.starts_with('r') && ext.len() > 1 && ext[1..].chars().all(|c| c.is_ascii_digit()));
    if !is_rar_ext {
        return Ok(None);
    }

    let parent = file_path
        .parent()
        .ok_or_else(|| DomainError::StorageError("No parent directory".to_string()))?;

    // Derive base_name from .rar suffix or .rNN suffix
    let base_name = if let Some(base) = file_name.strip_suffix(".rar") {
        base
    } else {
        // Strip .rNN suffix (e.g., "archive.r00" → "archive")
        let re_rnn = Regex::new(r"^(.+)\.r\d+$")
            .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;
        match re_rnn.captures(file_name) {
            Some(caps) => caps.get(1).map_or("", |m| m.as_str()),
            None => return Ok(None),
        }
    };

    if base_name.is_empty() {
        return Ok(None);
    }

    // Collect .rar + all .rNN parts
    let re = Regex::new(&format!(r"^{}\.(?:rar|r\d+)$", regex::escape(base_name)))
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    let mut parts = Vec::new();
    scan_directory(parent, &mut parts, |name| re.is_match(name))?;

    if parts.len() > 1 {
        sort_parts_numerically(&mut parts);
        return Ok(Some(parts));
    }

    Ok(None)
}

/// Detect 7z multi-part archive segments.
fn detect_7z(file_path: &Path, file_name: &str) -> Result<Option<Vec<PathBuf>>, DomainError> {
    let re = Regex::new(r"^(.+)\.7z\.(\d+)$")
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    if let Some(caps) = re.captures(file_name) {
        let base_name = &caps[1];
        let parent = file_path
            .parent()
            .ok_or_else(|| DomainError::StorageError("No parent directory".to_string()))?;

        return scan_parts(parent, base_name, ".7z.", "");
    }

    Ok(None)
}

/// Detect ZIP multi-part archive segments.
fn detect_zip(file_path: &Path, file_name: &str) -> Result<Option<Vec<PathBuf>>, DomainError> {
    let parent = file_path
        .parent()
        .ok_or_else(|| DomainError::StorageError("No parent directory".to_string()))?;

    // Try zip.001, zip.002 format
    let re_zip = Regex::new(r"^(.+)\.zip\.(\d+)$")
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    if let Some(caps) = re_zip.captures(file_name) {
        let base_name = &caps[1];
        return scan_parts(parent, base_name, ".zip.", "");
    }

    // Try z01, z02 format (also handle terminal .zip input)
    let re_z = Regex::new(r"^(.+)\.z(\d+)$")
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    // Derive base_name from either .zNN or .zip input
    let base_name_for_z = re_z
        .captures(file_name)
        .map(|caps| caps[1].to_string())
        .or_else(|| file_name.strip_suffix(".zip").map(|b| b.to_string()));

    if let Some(base_name) = base_name_for_z {
        let mut parts = Vec::new();
        let pattern = Regex::new(&format!(r"^{}\.z\d+$", regex::escape(&base_name)))
            .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;
        scan_directory(parent, &mut parts, |name| pattern.is_match(name))?;

        if !parts.is_empty() {
            // Include the terminal .zip file
            let zip_path = parent.join(format!("{}.zip", base_name));
            if zip_path.exists() {
                parts.push(zip_path);
            }
            sort_parts_numerically(&mut parts);
            return Ok(Some(parts));
        }
    }

    Ok(None)
}

/// Scan for multi-part archive segments matching a pattern.
fn scan_parts(
    parent: &Path,
    base_name: &str,
    separator: &str,
    suffix: &str,
) -> Result<Option<Vec<PathBuf>>, DomainError> {
    let re = Regex::new(&format!(
        r"^{}{}(\d+){}$",
        regex::escape(base_name),
        regex::escape(separator),
        regex::escape(suffix)
    ))
    .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    let mut parts = Vec::new();
    scan_directory(parent, &mut parts, |name| re.is_match(name))?;

    if parts.is_empty() {
        return Ok(None);
    }

    sort_parts_numerically(&mut parts);
    Ok(Some(parts))
}

/// Scan directory for entries matching a predicate.
fn scan_directory<F>(
    parent: &Path,
    parts: &mut Vec<PathBuf>,
    predicate: F,
) -> Result<(), DomainError>
where
    F: Fn(&str) -> bool,
{
    for entry in std::fs::read_dir(parent)
        .map_err(|e| DomainError::StorageError(format!("Failed to read directory: {}", e)))?
    {
        let entry = entry.map_err(|e| {
            DomainError::StorageError(format!("Failed to read directory entry: {}", e))
        })?;

        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
            && predicate(file_name)
        {
            parts.push(path);
        }
    }

    Ok(())
}

/// Sort archive parts in numerical order by extracted part numbers.
///
/// Files without a numeric part (e.g., `.rar` in a legacy set) sort first.
/// Terminal `.zip` segments (no digits in extension) sort last.
fn sort_parts_numerically(parts: &mut [PathBuf]) {
    parts.sort_by(|a, b| {
        let key = |p: &PathBuf| -> i64 {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let ext = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            match extract_part_number(name) {
                Some(n) => i64::from(n),
                // .rar without digits → sort first (-1)
                // .zip without digits → sort last (i64::MAX)
                None if ext.eq_ignore_ascii_case("zip") => i64::MAX,
                None => -1,
            }
        };
        key(a).cmp(&key(b))
    });
}

/// Check if a string looks like an archive part-numbering pattern.
///
/// Matches: "part01", "part1", "001", "01", "1" (pure digits or "part" prefix).
/// Does NOT match: "game2", "v3", "data" (regular name fragments).
fn is_part_pattern(s: &str) -> bool {
    if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
        return true;
    }
    if let Some(rest) = s.strip_prefix("part") {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

/// Cached regex for digit extraction.
fn digit_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d+)").unwrap())
}

/// Extract the trailing part number from an archive filename.
///
/// Extracts the *last* digit run to avoid confusion when the
/// base name itself contains digits (e.g., "game2.part03.rar" → 03).
fn extract_part_number(file_name: &str) -> Option<u32> {
    digit_regex()
        .find_iter(file_name)
        .last()
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_rar_segments() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create part files
        std::fs::write(base_path.join("archive.part01.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part02.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part03.rar"), "").unwrap();

        let file_path = base_path.join("archive.part01.rar");
        let result = detect_segments(&file_path).unwrap();

        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[0].file_name().unwrap().to_str().unwrap(),
            "archive.part01.rar"
        );
        assert_eq!(
            parts[1].file_name().unwrap().to_str().unwrap(),
            "archive.part02.rar"
        );
        assert_eq!(
            parts[2].file_name().unwrap().to_str().unwrap(),
            "archive.part03.rar"
        );
    }

    #[test]
    fn test_detect_7z_segments() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create 7z part files
        std::fs::write(base_path.join("archive.7z.001"), "").unwrap();
        std::fs::write(base_path.join("archive.7z.002"), "").unwrap();

        let file_path = base_path.join("archive.7z.001");
        let result = detect_segments(&file_path).unwrap();

        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn test_detect_zip_segments() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create zip part files
        std::fs::write(base_path.join("archive.zip.001"), "").unwrap();
        std::fs::write(base_path.join("archive.zip.002"), "").unwrap();

        let file_path = base_path.join("archive.zip.001");
        let result = detect_segments(&file_path).unwrap();

        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn test_single_file_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create a single archive file
        std::fs::write(base_path.join("archive.rar"), "").unwrap();

        let file_path = base_path.join("archive.rar");
        let result = detect_segments(&file_path).unwrap();

        // Single file with no parts should return None
        assert!(result.is_none());
    }

    #[test]
    fn test_verify_missing_parts() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create only part 1 and 3, missing part 2
        std::fs::write(base_path.join("archive.part01.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part03.rar"), "").unwrap();

        let parts = vec![
            base_path.join("archive.part01.rar"),
            base_path.join("archive.part03.rar"),
        ];

        let result = verify_all_parts_present(&parts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing"));
    }

    #[test]
    fn test_verify_all_parts_present_success() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create all parts
        std::fs::write(base_path.join("archive.part01.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part02.rar"), "").unwrap();

        let parts = vec![
            base_path.join("archive.part01.rar"),
            base_path.join("archive.part02.rar"),
        ];

        let result = verify_all_parts_present(&parts);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_numerical_sorting() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create parts in reverse order
        std::fs::write(base_path.join("archive.part10.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part02.rar"), "").unwrap();
        std::fs::write(base_path.join("archive.part01.rar"), "").unwrap();

        let file_path = base_path.join("archive.part02.rar");
        let result = detect_segments(&file_path).unwrap();

        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 3);
        // Verify numerical sort order
        assert_eq!(
            parts[0].file_name().unwrap().to_str().unwrap(),
            "archive.part01.rar"
        );
        assert_eq!(
            parts[1].file_name().unwrap().to_str().unwrap(),
            "archive.part02.rar"
        );
        assert_eq!(
            parts[2].file_name().unwrap().to_str().unwrap(),
            "archive.part10.rar"
        );
    }

    #[test]
    fn test_detect_legacy_rar_segments() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create legacy RAR files: archive.rar, archive.r00, archive.r01
        std::fs::write(base.join("archive.rar"), b"").unwrap();
        std::fs::write(base.join("archive.r00"), b"").unwrap();
        std::fs::write(base.join("archive.r01"), b"").unwrap();

        let result = detect_segments(&base.join("archive.rar")).unwrap();
        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 3);

        // .rar sorts first (terminal), then .r00, .r01
        let names: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.file_name()?.to_str())
            .collect();
        assert_eq!(names[0], "archive.rar");
        assert_eq!(names[1], "archive.r00");
        assert_eq!(names[2], "archive.r01");
    }

    #[test]
    fn test_detect_legacy_rar_from_r00_entrypoint() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        std::fs::write(base.join("archive.rar"), b"").unwrap();
        std::fs::write(base.join("archive.r00"), b"").unwrap();
        std::fs::write(base.join("archive.r01"), b"").unwrap();

        // Detecting from .r00 should find the same set
        let result = detect_segments(&base.join("archive.r00")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_detect_z_format_zip_segments() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        std::fs::write(base.join("archive.z01"), b"").unwrap();
        std::fs::write(base.join("archive.z02"), b"").unwrap();
        std::fs::write(base.join("archive.zip"), b"").unwrap();

        let result = detect_segments(&base.join("archive.z01")).unwrap();
        assert!(result.is_some());
        let parts = result.unwrap();
        assert_eq!(parts.len(), 3);

        // .z01, .z02 first (numerically sorted), .zip last (terminal)
        let names: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.file_name()?.to_str())
            .collect();
        assert_eq!(names[0], "archive.z01");
        assert_eq!(names[1], "archive.z02");
        assert_eq!(names[2], "archive.zip");
    }

    #[test]
    fn test_detect_z_format_from_zip_entrypoint() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        std::fs::write(base.join("archive.z01"), b"").unwrap();
        std::fs::write(base.join("archive.z02"), b"").unwrap();
        std::fs::write(base.join("archive.zip"), b"").unwrap();

        // Detecting from .zip should find the .zNN siblings
        let result = detect_segments(&base.join("archive.zip")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }
}
