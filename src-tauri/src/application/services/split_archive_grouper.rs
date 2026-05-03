//! Auto-group resolved split-archive parts into a [`Package`].
//!
//! When the Link Grabber resolves a batch that contains multiple parts
//! of the same split archive (e.g. `movie.part01.rar`, `movie.part02.rar`,
//! …), this grouper clusters them by base name and ensures one
//! [`Package`](crate::domain::model::package::Package) holds every part.
//! Re-resolving the same set must reuse the previously-created package
//! instead of producing a duplicate (PRD-v2 §P1.12).
//!
//! The grouper is the single point of truth for that idempotency: it
//! looks up the package by its `external_id`
//! (`split-archive:{format_tag}:{base}`) and either returns the
//! existing one or creates a new one. The format tag is part of the
//! key so a RAR set and a ZIP set sharing a base name produce two
//! distinct packages. The caller (the resolver / Link Grabber
//! pipeline) then attaches the resolved items by id once the
//! downloads have been persisted.
//!
//! Domain-pure: no plugin loader, no IPC, no HTTP. Just `PackageRepository`
//! + `EventBus`. Tests run entirely in-memory.
//!
//! # Detected formats
//!
//! - Modern RAR — `name.part01.rar`, `name.part02.rar`, …
//! - Legacy RAR — `name.r00`, `name.r01`, … (terminal `name.rar` joins the same set)
//! - 7z split  — `name.7z.001`, `name.7z.002`, …
//! - Zip split — `name.zip.001`, `name.zip.002`, …
//! - Tarball split — `name.tar.gz.001`, `name.tar.bz2.001`, `name.tar.xz.001`
//!
//! Files that do not match any pattern are returned untouched by
//! [`SplitArchiveGrouper::group_all`], which keeps non-archive links
//! flowing through the resolver as before.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use regex::Regex;
use uuid::Uuid;

use crate::application::error::AppError;
use crate::application::services::group_lock::acquire_grouper_lock;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::{Package, PackageId, PackageSourceType};
use crate::domain::ports::driven::{EventBus, PackageRepository};

/// Stable namespace prefix used for the `external_id` natural key of
/// split-archive packages. Prevents collisions with playlist packages
/// (which use raw `playlist_id`s) and lets the SQLite UNIQUE index
/// reject cross-process duplicates. The full key embeds the format
/// after the prefix (`split-archive:{format_tag}:{base}`) so two
/// archives that share a base name but use different formats (a RAR
/// set and a ZIP set both called `mix`) end up in distinct packages.
const EXTERNAL_ID_PREFIX: &str = "split-archive:";

/// Minimum number of detected parts required before the grouper bothers
/// creating a package. A lone `.part01.rar` is more useful as a regular
/// download than as an empty package shell — the user can still add the
/// other parts to it later via the package detail view.
const MIN_PARTS_TO_GROUP: usize = 2;

/// Upper bound on the number of links accepted by a single grouping
/// call. Mirrors `MAX_URLS` in
/// [`crate::application::commands::resolve_links`]: keeps a malicious
/// or accidental million-link payload from allocating an unbounded
/// `BTreeMap` worth of cluster state.
pub const MAX_LINKS: usize = 500;

/// One archive format the grouper recognises. Carried alongside the
/// detected base name so the missing-part error message can render the
/// right suffix (`part05.rar` vs `7z.005`) and the `external_id` can
/// distinguish a RAR set from a ZIP set sharing the same base name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SplitArchiveFormat {
    /// Modern RAR — `name.part01.rar`.
    PartRar,
    /// Legacy RAR — `name.r00`, `name.r01`, … plus the terminal `.rar`
    /// header file. The header is treated as part `0` for continuity.
    LegacyRar,
    /// 7z multi-volume — `name.7z.001`.
    SevenZ,
    /// Split ZIP using the `.zip.NNN` convention.
    Zip,
    /// Gzip tarball split — `name.tar.gz.001`.
    TarGz,
    /// Bzip2 tarball split — `name.tar.bz2.001`.
    TarBz2,
    /// XZ tarball split — `name.tar.xz.001`.
    TarXz,
}

impl SplitArchiveFormat {
    /// Suffix the user would type (e.g. `"part05.rar"`, `"7z.003"`),
    /// surfaced in `missing_parts` and the matching
    /// [`DomainEvent::SplitArchiveIncomplete`] event.
    ///
    /// Legacy RAR uses 0-based suffixes on disk (`r00`, `r01`, …) but
    /// we store as 1-based part numbers internally so every format
    /// shares the same numbering: detection adds 1 (`r00` → part 1),
    /// rendering subtracts 1 (part 1 → `r00`).
    fn part_suffix(self, part_num: u32) -> String {
        match self {
            Self::PartRar => format!("part{:02}.rar", part_num),
            Self::LegacyRar => {
                if part_num == 0 {
                    "rar".to_string()
                } else {
                    format!("r{:02}", part_num.saturating_sub(1))
                }
            }
            Self::SevenZ => format!("7z.{:03}", part_num),
            Self::Zip => format!("zip.{:03}", part_num),
            Self::TarGz => format!("tar.gz.{:03}", part_num),
            Self::TarBz2 => format!("tar.bz2.{:03}", part_num),
            Self::TarXz => format!("tar.xz.{:03}", part_num),
        }
    }

    /// Stable, URL-safe tag used inside the package `external_id`.
    /// Distinct values across formats are required so a RAR set and a
    /// ZIP set sharing a base name end up in two different packages
    /// instead of silently colliding under the same external_id.
    fn as_tag(self) -> &'static str {
        match self {
            Self::PartRar => "part-rar",
            Self::LegacyRar => "legacy-rar",
            Self::SevenZ => "7z",
            Self::Zip => "zip",
            Self::TarGz => "tar-gz",
            Self::TarBz2 => "tar-bz2",
            Self::TarXz => "tar-xz",
        }
    }
}

/// One inbound link sent to [`SplitArchiveGrouper::group_all`]. The
/// caller pre-extracts the URL filename (e.g. via the URL path's last
/// segment) so the grouper does not have to parse URLs itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitArchiveLink {
    pub url: String,
    pub filename: String,
}

/// Outcome of grouping for a single detected base name. The caller uses
/// `package_id` to attach the matched downloads via
/// `PackageRepository::attach_download`. `missing_parts` is non-empty
/// when one or more numbered parts are absent from the inbound batch —
/// the grouper also emits a [`DomainEvent::SplitArchiveIncomplete`] in
/// that case so the UI can notify the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitArchiveGroupResult {
    pub package_id: PackageId,
    pub base_name: String,
    pub package_name: String,
    pub created: bool,
    /// URLs from the input batch that belong to this group, ordered by
    /// detected part number so the caller can reproduce the visual order
    /// expected by the Link Grabber preview.
    pub urls: Vec<String>,
    /// Human-readable suffixes of the parts that should exist between
    /// part 1 and the highest detected part number but are missing from
    /// the input batch. Empty when the batch is contiguous.
    pub missing_parts: Vec<String>,
}

/// Detection output for a single filename — internal carrier between
/// `detect_from_filename` and the cluster builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedPart {
    pub base: String,
    pub part_num: u32,
    pub format: SplitArchiveFormat,
}

/// Try every supported pattern in order and return the first match.
/// Order matters: the more specific tarball patterns must be tried
/// before the generic `.7z.NNN` / `.zip.NNN` matchers so
/// `archive.tar.gz.001` is not mis-classified as a 7z volume.
pub(crate) fn detect_from_filename(file_name: &str) -> Option<DetectedPart> {
    if let Some(part) = match_part_rar(file_name) {
        return Some(part);
    }
    if let Some(part) = match_tar_split(file_name) {
        return Some(part);
    }
    if let Some(part) = match_seven_z(file_name) {
        return Some(part);
    }
    if let Some(part) = match_zip_split(file_name) {
        return Some(part);
    }
    if let Some(part) = match_legacy_rar(file_name) {
        return Some(part);
    }
    if let Some(part) = match_legacy_rar_header(file_name) {
        return Some(part);
    }
    None
}

fn match_part_rar(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(?P<base>.+?)\.part(?P<num>\d+)\.rar$").unwrap());
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    let part_num = caps.name("num")?.as_str().parse::<u32>().ok()?;
    Some(DetectedPart {
        base,
        part_num,
        format: SplitArchiveFormat::PartRar,
    })
}

fn match_tar_split(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^(?P<base>.+?)\.tar\.(?P<comp>gz|bz2|xz)\.(?P<num>\d{3})$").unwrap()
    });
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    let part_num = caps.name("num")?.as_str().parse::<u32>().ok()?;
    let format = match caps.name("comp")?.as_str() {
        "gz" => SplitArchiveFormat::TarGz,
        "bz2" => SplitArchiveFormat::TarBz2,
        "xz" => SplitArchiveFormat::TarXz,
        _ => return None,
    };
    Some(DetectedPart {
        base,
        part_num,
        format,
    })
}

fn match_seven_z(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(?P<base>.+?)\.7z\.(?P<num>\d{3})$").unwrap());
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    let part_num = caps.name("num")?.as_str().parse::<u32>().ok()?;
    Some(DetectedPart {
        base,
        part_num,
        format: SplitArchiveFormat::SevenZ,
    })
}

fn match_zip_split(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(?P<base>.+?)\.zip\.(?P<num>\d{3})$").unwrap());
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    let part_num = caps.name("num")?.as_str().parse::<u32>().ok()?;
    Some(DetectedPart {
        base,
        part_num,
        format: SplitArchiveFormat::Zip,
    })
}

fn match_legacy_rar(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Match `name.r00`, `name.r01`, …. The trailing digits are 2+ wide so
    // we don't accidentally pick up names that just happen to end in
    // `.r1` (which would be an unusual archive convention anyway).
    let re = RE.get_or_init(|| Regex::new(r"^(?P<base>.+?)\.r(?P<num>\d{2,})$").unwrap());
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    let raw_num = caps.name("num")?.as_str().parse::<u32>().ok()?;
    // Translate `.r00` → part 1, `.r01` → part 2, … so the legacy set
    // shares the same 1-based numbering as the modern formats. The
    // optional terminal `.rar` header file is treated as part 0 by
    // [`match_legacy_rar_header`].
    Some(DetectedPart {
        base,
        part_num: raw_num + 1,
        format: SplitArchiveFormat::LegacyRar,
    })
}

fn match_legacy_rar_header(file_name: &str) -> Option<DetectedPart> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // The terminal `.rar` header in a legacy multi-volume set
    // (`name.rar` + `name.r00` + `name.r01`…). Tried last in
    // [`detect_from_filename`] so the more specific patterns
    // (`name.partNN.rar`, `name.rNN`) win first. A standalone `.rar`
    // (no companion `.rNN`) survives detection but gets dropped by
    // [`MIN_PARTS_TO_GROUP`], so it does not produce a spurious
    // singleton package.
    let re = RE.get_or_init(|| Regex::new(r"^(?P<base>.+?)\.rar$").unwrap());
    let caps = re.captures(file_name)?;
    let base = caps.name("base")?.as_str().to_string();
    Some(DetectedPart {
        base,
        part_num: 0,
        format: SplitArchiveFormat::LegacyRar,
    })
}

pub struct SplitArchiveGrouper {
    repo: Arc<dyn PackageRepository>,
    event_bus: Arc<dyn EventBus>,
}

impl SplitArchiveGrouper {
    pub fn new(repo: Arc<dyn PackageRepository>, event_bus: Arc<dyn EventBus>) -> Self {
        Self { repo, event_bus }
    }

    /// Cluster `links` by detected base name + format and create /
    /// reuse one [`Package`] per cluster. Links that do not match any
    /// split-archive pattern are silently dropped from the result — the
    /// caller is expected to handle them through the regular resolver
    /// path. Clusters with fewer than [`MIN_PARTS_TO_GROUP`] detected
    /// parts are also dropped (a singleton is more useful as a
    /// stand-alone download than as a half-empty package).
    ///
    /// Returns `AppError::Validation` when `links.len()` exceeds
    /// [`MAX_LINKS`] so a runaway IPC payload cannot allocate
    /// unbounded cluster state.
    pub fn group_all(
        &self,
        links: &[SplitArchiveLink],
        created_at_ms: u64,
    ) -> Result<Vec<SplitArchiveGroupResult>, AppError> {
        if links.len() > MAX_LINKS {
            return Err(AppError::Validation(format!(
                "Too many links: {} (max {MAX_LINKS})",
                links.len()
            )));
        }

        // `BTreeMap` keeps the output deterministic (alphabetical),
        // which matters for snapshot tests and Link-Grabber preview.
        let mut clusters: BTreeMap<(String, SplitArchiveFormat), Vec<(u32, String)>> =
            BTreeMap::new();
        for link in links {
            let trimmed = link.filename.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(detected) = detect_from_filename(trimmed) {
                clusters
                    .entry((detected.base, detected.format))
                    .or_default()
                    .push((detected.part_num, link.url.clone()));
            }
        }

        let mut out = Vec::new();
        for ((base, format), mut parts) in clusters {
            if parts.len() < MIN_PARTS_TO_GROUP {
                continue;
            }
            parts.sort_by_key(|(n, _)| *n);
            let result = self.group_one_base(&base, format, &parts, created_at_ms)?;
            out.push(result);
        }
        Ok(out)
    }

    fn group_one_base(
        &self,
        base: &str,
        format: SplitArchiveFormat,
        sorted_parts: &[(u32, String)],
        created_at_ms: u64,
    ) -> Result<SplitArchiveGroupResult, AppError> {
        let trimmed_base = base.trim();
        if trimmed_base.is_empty() {
            return Err(AppError::Validation(
                "split-archive base name must not be empty".into(),
            ));
        }
        // Format is part of the natural key: a RAR set and a ZIP set
        // sharing the same base name must produce two distinct packages.
        let external_id = format!("{EXTERNAL_ID_PREFIX}{}:{trimmed_base}", format.as_tag());
        let urls: Vec<String> = sorted_parts.iter().map(|(_, u)| u.clone()).collect();
        let missing = compute_missing_parts(format, sorted_parts);

        // Hold the lock only across the find-then-save sequence; drop
        // it before publishing events so synchronous subscribers cannot
        // block other concurrent grouping calls.
        let (package_id, package_name, created) = {
            let _guard = acquire_grouper_lock();

            if let Some(existing) = self.repo.find_by_external_id(&external_id)? {
                (existing.id().clone(), existing.name().to_string(), false)
            } else {
                let new_id = PackageId::new(Uuid::new_v4().to_string());
                let mut package = Package::new(
                    new_id.clone(),
                    trimmed_base.to_string(),
                    PackageSourceType::SplitArchive,
                    created_at_ms,
                );
                package.set_external_id(Some(external_id.clone()));

                match self.repo.save(&package) {
                    Ok(()) => (new_id, trimmed_base.to_string(), true),
                    Err(save_err) => {
                        // Cross-process race: another writer inserted the
                        // same `external_id` between our `find` and
                        // `save`. Re-query and surface the winner as a
                        // reuse instead of bubbling the UNIQUE error.
                        if let Some(existing) = self.repo.find_by_external_id(&external_id)? {
                            (existing.id().clone(), existing.name().to_string(), false)
                        } else {
                            return Err(save_err.into());
                        }
                    }
                }
            }
        };

        if created {
            self.event_bus.publish(DomainEvent::PackageCreated {
                id: package_id.clone(),
                name: package_name.clone(),
            });
        }
        if !missing.is_empty() {
            self.event_bus.publish(DomainEvent::SplitArchiveIncomplete {
                package_id: package_id.clone(),
                base_name: trimmed_base.to_string(),
                missing_parts: missing.clone(),
            });
        }

        Ok(SplitArchiveGroupResult {
            package_id,
            base_name: trimmed_base.to_string(),
            package_name,
            created,
            urls,
            missing_parts: missing,
        })
    }
}

/// Walk `parts` (sorted ascending by part number) from the format's
/// natural baseline up to the highest seen number, emitting a
/// human-readable suffix for every gap. Legacy RAR starts at 0 because
/// the terminal `.rar` header is part 0; all other supported formats
/// are 1-based.
fn compute_missing_parts(
    format: SplitArchiveFormat,
    sorted_parts: &[(u32, String)],
) -> Vec<String> {
    if sorted_parts.is_empty() {
        return Vec::new();
    }
    let max = sorted_parts.last().map(|(n, _)| *n).unwrap_or(0);
    let present: std::collections::HashSet<u32> = sorted_parts.iter().map(|(n, _)| *n).collect();
    let start = match format {
        SplitArchiveFormat::LegacyRar => 0,
        _ => 1,
    };
    let mut missing = Vec::new();
    for n in start..=max {
        if !present.contains(&n) {
            missing.push(format.part_suffix(n));
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::application::commands::tests_support::{CapturingEventBus, InMemoryPackageRepo};
    use crate::domain::ports::driven::PackageRepository;

    fn arc_repo_and_bus() -> (Arc<InMemoryPackageRepo>, Arc<CapturingEventBus>) {
        (
            Arc::new(InMemoryPackageRepo::new()),
            Arc::new(CapturingEventBus::new()),
        )
    }

    fn link(url: &str, filename: &str) -> SplitArchiveLink {
        SplitArchiveLink {
            url: url.to_string(),
            filename: filename.to_string(),
        }
    }

    // ── Detection unit tests ────────────────────────────────────────

    #[test]
    fn test_detect_modern_rar_part() {
        let part = detect_from_filename("movie.part01.rar").expect("matches");
        assert_eq!(part.base, "movie");
        assert_eq!(part.part_num, 1);
        assert_eq!(part.format, SplitArchiveFormat::PartRar);
    }

    #[test]
    fn test_detect_modern_rar_three_digits() {
        let part = detect_from_filename("series.s01e01.part010.rar").expect("matches");
        assert_eq!(part.base, "series.s01e01");
        assert_eq!(part.part_num, 10);
    }

    #[test]
    fn test_detect_legacy_rar_r00_translates_to_part_one() {
        let part = detect_from_filename("backup.r00").expect("matches");
        assert_eq!(part.base, "backup");
        assert_eq!(part.part_num, 1);
        assert_eq!(part.format, SplitArchiveFormat::LegacyRar);
    }

    #[test]
    fn test_detect_legacy_rar_r10() {
        let part = detect_from_filename("backup.r10").expect("matches");
        assert_eq!(part.part_num, 11);
    }

    #[test]
    fn test_detect_seven_z() {
        let part = detect_from_filename("dump.7z.001").expect("matches");
        assert_eq!(part.base, "dump");
        assert_eq!(part.part_num, 1);
        assert_eq!(part.format, SplitArchiveFormat::SevenZ);
    }

    #[test]
    fn test_detect_zip_split() {
        let part = detect_from_filename("data.zip.005").expect("matches");
        assert_eq!(part.base, "data");
        assert_eq!(part.part_num, 5);
        assert_eq!(part.format, SplitArchiveFormat::Zip);
    }

    #[test]
    fn test_detect_tar_gz_split() {
        let part = detect_from_filename("logs.tar.gz.003").expect("matches");
        assert_eq!(part.base, "logs");
        assert_eq!(part.part_num, 3);
        assert_eq!(part.format, SplitArchiveFormat::TarGz);
    }

    #[test]
    fn test_detect_tar_bz2_split() {
        let part = detect_from_filename("logs.tar.bz2.002").expect("matches");
        assert_eq!(part.format, SplitArchiveFormat::TarBz2);
    }

    #[test]
    fn test_detect_tar_xz_split() {
        let part = detect_from_filename("logs.tar.xz.001").expect("matches");
        assert_eq!(part.format, SplitArchiveFormat::TarXz);
    }

    #[test]
    fn test_detect_returns_none_for_regular_filename() {
        assert!(detect_from_filename("photo.jpg").is_none());
        assert!(detect_from_filename("archive.zip").is_none());
        assert!(detect_from_filename("archive.7z").is_none());
        assert!(detect_from_filename("notes.tar.gz").is_none());
    }

    #[test]
    fn test_detect_legacy_rar_header_is_part_zero() {
        let part = detect_from_filename("backup.rar").expect("matches");
        assert_eq!(part.base, "backup");
        assert_eq!(part.part_num, 0);
        assert_eq!(part.format, SplitArchiveFormat::LegacyRar);
    }

    #[test]
    fn test_modern_part_rar_wins_over_legacy_header_match() {
        // `name.part01.rar` ends in `.rar` so the legacy-header regex
        // would also match — order in `detect_from_filename` must keep
        // PartRar primary so the part number is preserved.
        let part = detect_from_filename("movie.part01.rar").expect("matches");
        assert_eq!(part.format, SplitArchiveFormat::PartRar);
        assert_eq!(part.part_num, 1);
    }

    #[test]
    fn test_detect_does_not_pick_up_random_dot_r1_filename() {
        // Single-digit `.r1` is not a recognised RAR convention; skipping
        // it avoids false positives for filenames that happen to end in
        // `.r1` (e.g. a `.r1` config file).
        assert!(detect_from_filename("config.r1").is_none());
    }

    #[test]
    fn test_part_suffix_for_each_format() {
        assert_eq!(SplitArchiveFormat::PartRar.part_suffix(5), "part05.rar");
        assert_eq!(SplitArchiveFormat::PartRar.part_suffix(12), "part12.rar");
        assert_eq!(SplitArchiveFormat::SevenZ.part_suffix(3), "7z.003");
        assert_eq!(SplitArchiveFormat::Zip.part_suffix(7), "zip.007");
        assert_eq!(SplitArchiveFormat::TarGz.part_suffix(1), "tar.gz.001");
        assert_eq!(SplitArchiveFormat::TarBz2.part_suffix(2), "tar.bz2.002");
        assert_eq!(SplitArchiveFormat::TarXz.part_suffix(99), "tar.xz.099");
        // Legacy RAR: part 1 is `.r00`, part 2 is `.r01`, …
        assert_eq!(SplitArchiveFormat::LegacyRar.part_suffix(1), "r00");
        assert_eq!(SplitArchiveFormat::LegacyRar.part_suffix(11), "r10");
    }

    // ── Grouping integration tests ───────────────────────────────────

    fn ten_part_links(host: &str, base: &str) -> Vec<SplitArchiveLink> {
        (1..=10)
            .map(|n| {
                let name = format!("{base}.part{:02}.rar", n);
                let url = format!("https://{host}/{name}");
                link(&url, &name)
            })
            .collect()
    }

    #[test]
    fn test_group_all_creates_single_package_for_ten_part_archive() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = ten_part_links("ex.com", "movie");

        let results = grouper.group_all(&links, 1_700_000_000_000).expect("group");

        assert_eq!(
            results.len(),
            1,
            "ten matching parts must collapse to one package"
        );
        let r = &results[0];
        assert!(r.created);
        assert_eq!(r.base_name, "movie");
        assert_eq!(r.package_name, "movie");
        assert_eq!(r.urls.len(), 10);
        assert!(r.missing_parts.is_empty());

        // Persistence side: exactly one package row, with the expected
        // external_id namespace and `auto_extract` enabled so the
        // downstream extraction pipeline (PRD §7.2) auto-runs once every
        // part has finished downloading.
        let stored = repo.list().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source_type(), PackageSourceType::SplitArchive);
        assert_eq!(
            stored[0].external_id(),
            Some("split-archive:part-rar:movie")
        );
        assert!(
            stored[0].auto_extract(),
            "split-archive packages must default to auto_extract=true so the \
             completed package is extracted without an extra user click"
        );

        // Bus side: PackageCreated fired exactly once, no incomplete event.
        let snap = bus.snapshot();
        assert_eq!(
            snap.iter()
                .filter(|e| matches!(e, DomainEvent::PackageCreated { .. }))
                .count(),
            1
        );
        assert!(
            !snap
                .iter()
                .any(|e| matches!(e, DomainEvent::SplitArchiveIncomplete { .. })),
            "no incomplete event when batch is contiguous"
        );
    }

    #[test]
    fn test_group_all_emits_incomplete_when_part_is_missing() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        // Drop part 5 from the 10-part set.
        let links: Vec<SplitArchiveLink> = ten_part_links("ex.com", "movie")
            .into_iter()
            .filter(|l| !l.filename.contains("part05"))
            .collect();

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.urls.len(), 9);
        assert_eq!(r.missing_parts, vec!["part05.rar".to_string()]);

        let snap = bus.snapshot();
        let incomplete: Vec<&DomainEvent> = snap
            .iter()
            .filter(|e| matches!(e, DomainEvent::SplitArchiveIncomplete { .. }))
            .collect();
        assert_eq!(incomplete.len(), 1);
        if let DomainEvent::SplitArchiveIncomplete {
            base_name,
            missing_parts,
            ..
        } = incomplete[0]
        {
            assert_eq!(base_name, "movie");
            assert_eq!(missing_parts, &vec!["part05.rar".to_string()]);
        } else {
            panic!("wrong event variant");
        }
    }

    #[test]
    fn test_group_all_handles_multiple_bases_in_one_batch() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());

        let mut links = ten_part_links("ex.com", "alpha");
        links.extend(ten_part_links("ex.com", "bravo"));

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.base_name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"bravo"));
        assert_eq!(repo.list().unwrap().len(), 2);
    }

    #[test]
    fn test_group_all_skips_singleton_part() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = vec![link("https://ex.com/lone.part01.rar", "lone.part01.rar")];

        let results = grouper.group_all(&links, 0).expect("group");
        assert!(
            results.is_empty(),
            "single part should not create a package"
        );
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn test_group_all_ignores_non_archive_links() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = vec![
            link("https://ex.com/photo.jpg", "photo.jpg"),
            link("https://ex.com/dump.zip", "dump.zip"),
        ];

        let results = grouper.group_all(&links, 0).expect("group");
        assert!(results.is_empty());
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn test_group_all_is_idempotent_on_re_resolve() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = ten_part_links("ex.com", "movie");

        let first = grouper.group_all(&links, 0).expect("first");
        let second = grouper.group_all(&links, 0).expect("second");

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(first[0].created);
        assert!(
            !second[0].created,
            "re-resolve must reuse the existing package"
        );
        assert_eq!(first[0].package_id, second[0].package_id);
        assert_eq!(repo.list().unwrap().len(), 1, "no duplicate package");

        let created_events = bus
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::PackageCreated { .. }))
            .count();
        assert_eq!(
            created_events, 1,
            "PackageCreated must fire only on first creation"
        );
    }

    #[test]
    fn test_group_all_reuse_still_emits_incomplete_when_parts_missing() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        // First resolve has all parts.
        let _ = grouper
            .group_all(&ten_part_links("ex.com", "movie"), 0)
            .unwrap();
        // Drain bus events from the first run so the second run's
        // assertions are unambiguous.
        let _ = bus.snapshot();

        // Re-resolve with a missing part 7.
        let partial: Vec<SplitArchiveLink> = ten_part_links("ex.com", "movie")
            .into_iter()
            .filter(|l| !l.filename.contains("part07"))
            .collect();
        let results = grouper.group_all(&partial, 0).expect("reuse");

        assert_eq!(results.len(), 1);
        assert!(!results[0].created);
        assert_eq!(results[0].missing_parts, vec!["part07.rar".to_string()]);

        let incomplete = bus
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::SplitArchiveIncomplete { .. }))
            .count();
        // At least one incomplete event for the re-resolve. (The first run had none.)
        assert!(incomplete >= 1);
    }

    #[test]
    fn test_group_all_handles_seven_z_format_with_gap() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = vec![
            link("https://ex.com/dump.7z.001", "dump.7z.001"),
            link("https://ex.com/dump.7z.002", "dump.7z.002"),
            link("https://ex.com/dump.7z.004", "dump.7z.004"),
        ];

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].missing_parts, vec!["7z.003".to_string()]);
    }

    #[test]
    fn test_group_all_creates_distinct_packages_for_same_base_across_formats() {
        // A RAR set and a ZIP set sharing the same base name describe
        // two different archives — they must produce two packages, not
        // collapse under a single external_id.
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let mut links = ten_part_links("ex.com", "mix");
        links.push(link("https://ex.com/mix.zip.001", "mix.zip.001"));
        links.push(link("https://ex.com/mix.zip.002", "mix.zip.002"));

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 2);
        let stored = repo.list().unwrap();
        assert_eq!(stored.len(), 2, "RAR and ZIP must not share a package");

        let mut external_ids: Vec<String> = stored
            .iter()
            .filter_map(|p| p.external_id().map(str::to_string))
            .collect();
        external_ids.sort();
        assert_eq!(
            external_ids,
            vec![
                "split-archive:part-rar:mix".to_string(),
                "split-archive:zip:mix".to_string(),
            ]
        );
    }

    #[test]
    fn test_group_all_legacy_rar_includes_terminal_header() {
        // `backup.rar` + `backup.r00` + `backup.r01` is a valid legacy
        // 3-volume set. The header file (`backup.rar`) used to be
        // dropped because detection only matched `.rNN`, leaving the
        // cluster a singleton that fell below MIN_PARTS_TO_GROUP.
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus.clone());
        let links = vec![
            link("https://ex.com/backup.rar", "backup.rar"),
            link("https://ex.com/backup.r00", "backup.r00"),
            link("https://ex.com/backup.r01", "backup.r01"),
        ];

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 1, "all three volumes share one package");
        let r = &results[0];
        assert_eq!(r.urls.len(), 3);
        assert!(r.missing_parts.is_empty());
        assert_eq!(r.base_name, "backup");

        let stored = repo.list().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].external_id(),
            Some("split-archive:legacy-rar:backup")
        );
    }

    #[test]
    fn test_group_all_legacy_rar_reports_missing_header() {
        // Inverse of the previous test: `.r00` + `.r01` only — the
        // header (.rar, part 0) is reported as missing so the UI can
        // tell the user to fetch it.
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo, bus);
        let links = vec![
            link("https://ex.com/backup.r00", "backup.r00"),
            link("https://ex.com/backup.r01", "backup.r01"),
        ];

        let results = grouper.group_all(&links, 0).expect("group");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].missing_parts, vec!["rar".to_string()]);
    }

    #[test]
    fn test_group_all_drops_lone_legacy_rar_header() {
        // A standalone `.rar` (no `.rNN` companion) is just a regular
        // RAR archive, not a split set — MIN_PARTS_TO_GROUP must keep
        // it out of the package list so the resolver sends it through
        // the regular single-file path.
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo.clone(), bus);
        let links = vec![link("https://ex.com/lonely.rar", "lonely.rar")];

        let results = grouper.group_all(&links, 0).expect("group");
        assert!(results.is_empty());
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn test_group_all_caps_link_count_to_avoid_dos() {
        // The IPC entry-point can hand us an arbitrarily large batch;
        // the grouper must reject it instead of allocating unbounded
        // cluster state. Mirrors `MAX_URLS` in `resolve_links`.
        let (repo, bus) = arc_repo_and_bus();
        let grouper = SplitArchiveGrouper::new(repo, bus);

        let oversize: Vec<SplitArchiveLink> = (0..MAX_LINKS + 1)
            .map(|n| {
                let name = format!("file{n}.bin");
                link(&format!("https://ex.com/{name}"), &name)
            })
            .collect();

        let err = grouper
            .group_all(&oversize, 0)
            .expect_err("oversize batch must be rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
