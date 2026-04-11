use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use tracing::{debug, warn};

use crate::domain::error::DomainError;
use crate::domain::ports::driven::ClipboardObserver;

static URL_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(https?://|ftp://|magnet:\?)[^\s]+")
        .expect("URL regex is a compile-time constant")
});

/// Monitors the system clipboard for URLs using Tauri's clipboard plugin.
///
/// Polls every 500ms and detects new URLs via regex matching.
pub struct TauriClipboardObserver {
    app_handle: tauri::AppHandle,
    enabled: Arc<AtomicBool>,
    detected_urls: Arc<Mutex<Vec<String>>>,
    seen_urls: Arc<Mutex<HashSet<String>>>,
    last_content: Arc<Mutex<String>>,
}

impl TauriClipboardObserver {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            enabled: Arc::new(AtomicBool::new(false)),
            detected_urls: Arc::new(Mutex::new(Vec::new())),
            seen_urls: Arc::new(Mutex::new(HashSet::new())),
            last_content: Arc::new(Mutex::new(String::new())),
        }
    }

    fn extract_urls(text: &str) -> Vec<String> {
        URL_REGEX
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect()
    }
}

const MAX_SEEN_URLS: usize = 1000;
const MAX_CLIPBOARD_LEN: usize = 1_000_000;

impl ClipboardObserver for TauriClipboardObserver {
    fn start(&self) -> Result<(), DomainError> {
        // Re-entrancy guard: if already running, just ensure enabled
        if self.enabled.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let app_handle = self.app_handle.clone();
        let enabled = Arc::clone(&self.enabled);
        let detected_urls = Arc::clone(&self.detected_urls);
        let seen_urls = Arc::clone(&self.seen_urls);
        let last_content = Arc::clone(&self.last_content);

        tokio::spawn(async move {
            loop {
                if !enabled.load(Ordering::SeqCst) {
                    break;
                }

                let clipboard_text = tokio::task::spawn_blocking({
                    let handle = app_handle.clone();
                    move || {
                        use tauri_plugin_clipboard_manager::ClipboardExt;
                        handle.clipboard().read_text()
                    }
                })
                .await;

                if let Ok(Ok(content)) = clipboard_text {
                    // Skip very large clipboard content to avoid regex overhead
                    if content.len() > MAX_CLIPBOARD_LEN {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }

                    let mut last = last_content.lock().unwrap();
                    if content != *last {
                        *last = content.clone();
                        drop(last);

                        let urls = Self::extract_urls(&content);
                        if !urls.is_empty() {
                            let mut seen = seen_urls.lock().unwrap();

                            // Evict to prevent unbounded growth
                            if seen.len() > MAX_SEEN_URLS {
                                seen.clear();
                            }

                            let mut new_urls = Vec::new();

                            for url in urls {
                                if !seen.contains(&url) {
                                    seen.insert(url.clone());
                                    new_urls.push(url);
                                }
                            }

                            if !new_urls.is_empty() {
                                debug!(count = new_urls.len(), "clipboard: new URLs detected");
                                let mut buffer = detected_urls.lock().unwrap();
                                buffer.extend(new_urls);
                            }
                        }
                    }
                } else if let Ok(Err(e)) = clipboard_text {
                    warn!("clipboard: failed to read: {}", e);
                }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });

        Ok(())
    }

    fn stop(&self) -> Result<(), DomainError> {
        self.enabled.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn get_urls(&self) -> Result<Vec<String>, DomainError> {
        let urls = std::mem::take(&mut *self.detected_urls.lock().unwrap());
        Ok(urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_extraction_http() {
        let text = "Check this out: https://example.com/file.zip";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["https://example.com/file.zip"]);
    }

    #[test]
    fn test_url_extraction_multiple() {
        let text = "https://a.com http://b.com ftp://c.com";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://a.com".to_string()));
        assert!(urls.contains(&"http://b.com".to_string()));
        assert!(urls.contains(&"ftp://c.com".to_string()));
    }

    #[test]
    fn test_url_extraction_magnet() {
        let text = "Download: magnet:?xt=urn:btih:abc123";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["magnet:?xt=urn:btih:abc123"]);
    }

    #[test]
    fn test_duplicate_detection() {
        let text1 = "https://example.com/file.zip";
        let text2 = "https://example.com/file.zip https://other.com";

        let urls1 = TauriClipboardObserver::extract_urls(text1);
        let urls2 = TauriClipboardObserver::extract_urls(text2);

        assert_eq!(urls1, vec!["https://example.com/file.zip"]);
        assert_eq!(urls2.len(), 2);
    }
}
