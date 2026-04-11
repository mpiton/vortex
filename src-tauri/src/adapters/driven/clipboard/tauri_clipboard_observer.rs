use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::domain::error::DomainError;
use crate::domain::ports::driven::ClipboardObserver;

/// Greedy match — trailing punctuation is stripped by `strip_trailing_punctuation`.
static URL_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(https?://|ftp://|magnet:\?)\S+")
        .expect("URL regex is a compile-time constant")
});

const MAX_SEEN_URLS: usize = 1000;
const MAX_CLIPBOARD_LEN: usize = 1_000_000;

/// Monitors the system clipboard for URLs using Tauri's clipboard plugin.
///
/// Polls every 500ms and detects new URLs via regex matching.
pub struct TauriClipboardObserver {
    app_handle: tauri::AppHandle,
    enabled: Arc<AtomicBool>,
    detected_urls: Arc<Mutex<Vec<String>>>,
    seen_urls: Arc<Mutex<HashSet<String>>>,
    last_content: Arc<Mutex<String>>,
    task_handle: Mutex<Option<JoinHandle<()>>>,
}

impl TauriClipboardObserver {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            enabled: Arc::new(AtomicBool::new(false)),
            detected_urls: Arc::new(Mutex::new(Vec::new())),
            seen_urls: Arc::new(Mutex::new(HashSet::new())),
            last_content: Arc::new(Mutex::new(String::new())),
            task_handle: Mutex::new(None),
        }
    }

    fn extract_urls(text: &str) -> Vec<String> {
        URL_REGEX
            .find_iter(text)
            .map(|m| Self::strip_trailing_punctuation(m.as_str()).to_string())
            .collect()
    }

    /// Strips trailing punctuation that is likely wrapper syntax, not part of
    /// the URL. Keeps `]` when a matching `[` exists in the URL (IPv6 hosts).
    fn strip_trailing_punctuation(url: &str) -> &str {
        let bytes = url.as_bytes();
        let mut end = bytes.len();
        while end > 0 {
            match bytes[end - 1] {
                b'.' | b',' | b')' | b';' | b':' | b'>' | b'\'' | b'"' => {
                    end -= 1;
                }
                b']' if !url[..end].contains('[') => {
                    end -= 1;
                }
                _ => break,
            }
        }
        &url[..end]
    }
}

impl ClipboardObserver for TauriClipboardObserver {
    fn start(&self) -> Result<(), DomainError> {
        // Re-entrancy guard: if already running, just ensure enabled
        if self.enabled.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        // Abort any lingering task from a previous stop→start cycle
        let mut handle_guard = self.task_handle.lock().unwrap();
        if let Some(old_handle) = handle_guard.take() {
            old_handle.abort();
        }

        let app_handle = self.app_handle.clone();
        let enabled = Arc::clone(&self.enabled);
        let detected_urls = Arc::clone(&self.detected_urls);
        let seen_urls = Arc::clone(&self.seen_urls);
        let last_content = Arc::clone(&self.last_content);

        let handle = tokio::spawn(async move {
            loop {
                if !enabled.load(Ordering::SeqCst) {
                    break;
                }

                let clipboard_result = tokio::task::spawn_blocking({
                    let h = app_handle.clone();
                    move || {
                        use tauri_plugin_clipboard_manager::ClipboardExt;
                        h.clipboard().read_text()
                    }
                })
                .await;

                // Handle both JoinError (task panic) and clipboard errors
                let content = match clipboard_result {
                    Ok(Ok(text)) => text,
                    Ok(Err(e)) => {
                        warn!("clipboard: failed to read: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                    Err(e) => {
                        warn!("clipboard: spawn_blocking join error: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                };

                // Skip very large clipboard content to avoid regex overhead
                if content.len() > MAX_CLIPBOARD_LEN {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }

                let changed = {
                    let mut last = last_content.lock().unwrap();
                    if content != *last {
                        *last = content.clone();
                        true
                    } else {
                        false
                    }
                };

                if changed {
                    let urls = Self::extract_urls(&content);
                    if !urls.is_empty() {
                        let mut seen = seen_urls.lock().unwrap();

                        // Evict when at capacity to prevent unbounded growth
                        if seen.len() >= MAX_SEEN_URLS {
                            seen.clear();
                        }

                        let mut new_urls = Vec::new();
                        for url in urls {
                            if seen.insert(url.clone()) {
                                new_urls.push(url);
                            }
                        }
                        drop(seen);

                        if !new_urls.is_empty() {
                            debug!(count = new_urls.len(), "clipboard: new URLs detected");
                            let mut buffer = detected_urls.lock().unwrap();
                            buffer.extend(new_urls);
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });

        *handle_guard = Some(handle);
        Ok(())
    }

    fn stop(&self) -> Result<(), DomainError> {
        self.enabled.store(false, Ordering::SeqCst);

        // Abort the polling task immediately instead of waiting for it to notice the flag
        let mut handle_guard = self.task_handle.lock().unwrap();
        if let Some(handle) = handle_guard.take() {
            handle.abort();
        }

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
    fn test_url_extraction_strips_trailing_punctuation() {
        let text = "See https://example.com/file.zip.";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["https://example.com/file.zip"]);
    }

    #[test]
    fn test_url_extraction_strips_trailing_paren() {
        let text = "(https://example.com/file.zip)";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["https://example.com/file.zip"]);
    }

    #[test]
    fn test_url_extraction_multiple() {
        let text = "https://a.com/f http://b.com/g ftp://c.com/h";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://a.com/f".to_string()));
        assert!(urls.contains(&"http://b.com/g".to_string()));
        assert!(urls.contains(&"ftp://c.com/h".to_string()));
    }

    #[test]
    fn test_url_extraction_magnet() {
        let text = "Download: magnet:?xt=urn:btih:abc123";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["magnet:?xt=urn:btih:abc123"]);
    }

    #[test]
    fn test_url_extraction_strips_trailing_bracket() {
        let text = "[https://example.com/file.zip]";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["https://example.com/file.zip"]);
    }

    #[test]
    fn test_url_extraction_preserves_ipv6() {
        let text = "http://[::1]";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["http://[::1]"]);
    }

    #[test]
    fn test_url_extraction_preserves_ipv6_with_path() {
        let text = "http://[::1]:8080/path";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["http://[::1]:8080/path"]);
    }

    #[test]
    fn test_url_extraction_short_host() {
        let text = "tiny host: http://a";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert_eq!(urls, vec!["http://a"]);
    }

    #[test]
    fn test_url_extraction_no_match() {
        let text = "no urls here, just plain text";
        let urls = TauriClipboardObserver::extract_urls(text);
        assert!(urls.is_empty());
    }
}
