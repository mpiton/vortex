//! Split archive detection module.
//!
//! Detects and validates multi-part archive segments (RAR, 7z, ZIP) in the filesystem.
//! Supports various archive formats:
//! - RAR: `name.part01.rar`, `name.part02.rar`, ... or `name.rar`, `name.r00`, `name.r01`
//! - 7z: `name.7z.001`, `name.7z.002`, ...
//! - ZIP: `name.zip.001`, `name.zip.002`, ... or `name.z01`, `name.z02`, ...

use crate::domain::error::DomainError;
use regex::Regex;
use std::path::{Path, PathBuf};

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
            let prev_num = extract_part_number(parts[i - 1].file_name().unwrap().to_str().unwrap())
                .unwrap_or(0);
            let curr_num =
                extract_part_number(part.file_name().unwrap().to_str().unwrap()).unwrap_or(0);

            if curr_num != prev_num + 1 {
                missing.push(
                    part.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(format!("{:02}", prev_num + 1)),
                );
            }
        }
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
    if !file_name.ends_with(".rar")
        && !file_name
            .ends_with(|c: char| c.is_ascii_digit() && file_name.chars().rev().nth(1) == Some('r'))
    {
        return Ok(None);
    }

    let parent = file_path
        .parent()
        .ok_or_else(|| DomainError::StorageError("No parent directory".to_string()))?;

    // Check for .r00, .r01 pattern
    if let Some(base_name) = file_name.strip_suffix(".rar") {
        let re = Regex::new(&format!(r"^{}\.(r\d+)$", regex::escape(base_name)))
            .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

        let mut parts = Vec::new();
        scan_directory(parent, &mut parts, |name| re.is_match(name))?;

        if !parts.is_empty() {
            sort_parts_numerically(&mut parts);
            parts.insert(0, file_path.to_path_buf());
            return Ok(Some(parts));
        }

        // Check if there are any .r00/.r01 style parts
        if has_rar_parts(parent, base_name)? {
            let mut all_parts = vec![file_path.to_path_buf()];
            let re_parts = Regex::new(&format!(r"^{}\.(r\d+)$", regex::escape(base_name)))
                .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;
            scan_directory(parent, &mut all_parts, |name| re_parts.is_match(name))?;
            sort_parts_numerically(&mut all_parts);
            return Ok(Some(all_parts));
        }
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

    // Try z01, z02 format
    let re_z = Regex::new(r"^(.+)\.z(\d+)$")
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    if let Some(caps) = re_z.captures(file_name) {
        let base_name = &caps[1];
        let mut parts = Vec::new();
        let pattern = Regex::new(&format!(r"^{}\.z\d+$", regex::escape(base_name)))
            .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;
        scan_directory(parent, &mut parts, |name| pattern.is_match(name))?;

        if !parts.is_empty() {
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
fn sort_parts_numerically(parts: &mut [PathBuf]) {
    parts.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(extract_part_number)
            .unwrap_or(0)
    });
}

/// Extract the trailing part number from an archive filename.
///
/// Extracts the *last* digit run to avoid confusion when the
/// base name itself contains digits (e.g., "game2.part03.rar" → 03).
fn extract_part_number(file_name: &str) -> Option<u32> {
    let re = Regex::new(r"(\d+)[^/\\]*$").ok()?;
    re.captures(file_name)?[1].parse::<u32>().ok()
}

/// Check if RAR multi-part files exist for the given base name.
fn has_rar_parts(parent: &Path, base_name: &str) -> Result<bool, DomainError> {
    let re = Regex::new(&format!(r"^{}\.(r\d+)$", regex::escape(base_name)))
        .map_err(|e| DomainError::StorageError(format!("Regex error: {}", e)))?;

    for entry in std::fs::read_dir(parent)
        .map_err(|e| DomainError::StorageError(format!("Failed to read directory: {}", e)))?
    {
        let entry = entry.map_err(|e| {
            DomainError::StorageError(format!("Failed to read directory entry: {}", e))
        })?;

        if let Some(file_name) = entry.path().file_name().and_then(|n| n.to_str())
            && re.is_match(file_name)
        {
            return Ok(true);
        }
    }

    Ok(false)
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
}
