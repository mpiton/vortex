//! Helpers around the bounded `HistoryRepository::list` contract.
//!
//! The port caps each call at [`MAX_HISTORY_PAGE_SIZE`] so callers that
//! need the full table (history export, duplicate detection) all end up
//! re-implementing the same offset-bump loop. Centralising it here
//! keeps the contract in one place — every caller advances by exactly
//! one page until a short page surfaces.

use crate::domain::error::DomainError;
use crate::domain::model::views::HistoryEntry;
use crate::domain::ports::driven::HistoryRepository;
use crate::domain::ports::driven::history_repository::MAX_HISTORY_PAGE_SIZE;

/// Walk the entire history table page by page, invoking `f` once per
/// page. The callback owns each page and can drop / accumulate / index
/// without buffering the whole table in this helper.
pub fn for_each_history_page<F>(repo: &dyn HistoryRepository, mut f: F) -> Result<(), DomainError>
where
    F: FnMut(Vec<HistoryEntry>),
{
    let mut offset = 0usize;
    loop {
        let page = repo.list(None, None, Some(MAX_HISTORY_PAGE_SIZE), Some(offset))?;
        let len = page.len();
        f(page);
        if len < MAX_HISTORY_PAGE_SIZE {
            break;
        }
        offset += MAX_HISTORY_PAGE_SIZE;
    }
    Ok(())
}

/// Materialise the full history table by collecting every page into a
/// single `Vec`. Convenience wrapper around [`for_each_history_page`]
/// for callers (e.g. export) that genuinely need the whole table.
pub fn list_full_history(repo: &dyn HistoryRepository) -> Result<Vec<HistoryEntry>, DomainError> {
    let mut entries: Vec<HistoryEntry> = Vec::new();
    for_each_history_page(repo, |page| entries.extend(page))?;
    Ok(entries)
}
