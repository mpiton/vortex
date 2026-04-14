use std::collections::VecDeque;

use dashmap::DashMap;

#[derive(Debug)]
pub struct DownloadLogStore {
    max_entries_per_download: usize,
    lines_by_download: DashMap<u64, VecDeque<String>>,
}

impl DownloadLogStore {
    pub fn new(max_entries_per_download: usize) -> Self {
        Self {
            max_entries_per_download: max_entries_per_download.max(1),
            lines_by_download: DashMap::new(),
        }
    }

    pub fn push(&self, download_id: u64, line: String) {
        let mut lines = self.lines_by_download.entry(download_id).or_default();
        lines.push_back(line);
        if lines.len() > self.max_entries_per_download {
            lines.pop_front();
        }
    }

    pub fn remove(&self, download_id: u64) {
        self.lines_by_download.remove(&download_id);
    }

    pub fn recent(&self, download_id: u64, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }

        self.lines_by_download
            .get(&download_id)
            .map(|lines| {
                let start = lines.len().saturating_sub(limit);
                lines.iter().skip(start).cloned().collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::DownloadLogStore;

    #[test]
    fn keeps_only_the_most_recent_lines_for_a_download() {
        let store = DownloadLogStore::new(3);

        store.push(7, "[INFO] queued".to_string());
        store.push(7, "[INFO] started".to_string());
        store.push(7, "[WARN] retrying".to_string());
        store.push(7, "[INFO] resumed".to_string());

        assert_eq!(
            store.recent(7, 10),
            vec![
                "[INFO] started".to_string(),
                "[WARN] retrying".to_string(),
                "[INFO] resumed".to_string(),
            ]
        );
    }

    #[test]
    fn isolates_lines_by_download_id() {
        let store = DownloadLogStore::new(4);

        store.push(1, "[INFO] first".to_string());
        store.push(2, "[INFO] second".to_string());

        assert_eq!(store.recent(1, 10), vec!["[INFO] first".to_string()]);
        assert_eq!(store.recent(2, 10), vec!["[INFO] second".to_string()]);
    }

    #[test]
    fn removes_all_lines_for_a_download() {
        let store = DownloadLogStore::new(4);

        store.push(1, "[INFO] first".to_string());
        store.remove(1);

        assert!(store.recent(1, 10).is_empty());
    }
}
