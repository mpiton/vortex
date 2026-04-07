use crate::domain::model::download::DownloadId;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    id: u64,
    name: String,
    download_ids: Vec<DownloadId>,
    created_at: u64,
}

impl Package {
    pub fn new(id: u64, name: String) -> Self {
        Self {
            id,
            name,
            download_ids: Vec::new(),
            created_at: 0,
        }
    }

    pub fn add_download(&mut self, id: DownloadId) {
        if !self.download_ids.contains(&id) {
            self.download_ids.push(id);
        }
    }

    pub fn remove_download(&mut self, id: DownloadId) {
        self.download_ids.retain(|d| d != &id);
    }

    pub fn download_count(&self) -> usize {
        self.download_ids.len()
    }

    pub fn contains_download(&self, id: DownloadId) -> bool {
        self.download_ids.contains(&id)
    }

    pub fn downloads(&self) -> &[DownloadId] {
        &self.download_ids
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_package() -> Package {
        Package::new(1, "My Package".to_string())
    }

    #[test]
    fn test_package_new() {
        let p = make_package();
        assert_eq!(p.id(), 1);
        assert_eq!(p.name(), "My Package");
        assert_eq!(p.created_at(), 0);
        assert_eq!(p.download_count(), 0);
        assert!(p.downloads().is_empty());
    }

    #[test]
    fn test_package_add_download() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
        assert!(p.contains_download(DownloadId(10)));
    }

    #[test]
    fn test_package_add_download_duplicate_ignored() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        p.add_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
    }

    #[test]
    fn test_package_remove_download() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        p.add_download(DownloadId(20));
        p.remove_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
        assert!(!p.contains_download(DownloadId(10)));
        assert!(p.contains_download(DownloadId(20)));
    }

    #[test]
    fn test_package_remove_nonexistent() {
        let mut p = make_package();
        p.remove_download(DownloadId(99));
        assert_eq!(p.download_count(), 0);
    }

    #[test]
    fn test_package_download_count() {
        let mut p = make_package();
        assert_eq!(p.download_count(), 0);
        p.add_download(DownloadId(1));
        p.add_download(DownloadId(2));
        p.add_download(DownloadId(3));
        assert_eq!(p.download_count(), 3);
    }

    #[test]
    fn test_package_contains() {
        let mut p = make_package();
        assert!(!p.contains_download(DownloadId(5)));
        p.add_download(DownloadId(5));
        assert!(p.contains_download(DownloadId(5)));
    }
}
