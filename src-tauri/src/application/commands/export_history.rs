//! Export every history entry to a CSV or JSON file.

use std::fs;

use serde::Serialize;

use crate::application::command_bus::CommandBus;
use crate::application::commands::{ExportHistoryCommand, ExportHistoryFormat};
use crate::application::error::AppError;
use crate::domain::model::views::HistoryEntry;
use crate::domain::ports::driven::history_repository::MAX_HISTORY_PAGE_SIZE;

impl CommandBus {
    pub async fn handle_export_history(
        &self,
        cmd: ExportHistoryCommand,
    ) -> Result<usize, AppError> {
        // `list` is capped at MAX_HISTORY_PAGE_SIZE per the port contract, so
        // we page through it to produce a full export instead of silently
        // truncating the file at 500 rows.
        let mut entries: Vec<HistoryEntry> = Vec::new();
        let mut offset = 0usize;
        loop {
            let page =
                self.history_repo()
                    .list(None, None, Some(MAX_HISTORY_PAGE_SIZE), Some(offset))?;
            let len = page.len();
            entries.extend(page);
            if len < MAX_HISTORY_PAGE_SIZE {
                break;
            }
            offset += MAX_HISTORY_PAGE_SIZE;
        }

        let bytes = match cmd.format {
            ExportHistoryFormat::Csv => encode_csv(&entries).into_bytes(),
            ExportHistoryFormat::Json => encode_json(&entries)?.into_bytes(),
        };

        if let Some(parent) = cmd.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| AppError::Storage(e.to_string()))?;
        }
        fs::write(&cmd.path, bytes).map_err(|e| AppError::Storage(e.to_string()))?;
        Ok(entries.len())
    }
}

/// Escape a single CSV field per RFC 4180 and neutralise spreadsheet
/// formulas.
///
/// Fields containing a comma, double quote or newline are enclosed in double
/// quotes with embedded quotes doubled. On top of that, a value starting with
/// `=`, `+`, `-`, `@`, TAB or CR is prefixed with a single apostrophe so that
/// Excel / Google Sheets open the CSV as data instead of evaluating untrusted
/// input as a formula (the OWASP "CSV injection" mitigation).
fn escape_csv_field(value: &str) -> String {
    let needs_formula_guard = value
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let guarded = if needs_formula_guard {
        let mut owned = String::with_capacity(value.len() + 1);
        owned.push('\'');
        owned.push_str(value);
        owned
    } else {
        value.to_string()
    };
    let needs_quote = guarded.contains([',', '"', '\n', '\r']);
    if needs_quote {
        let escaped = guarded.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        guarded
    }
}

fn encode_csv(entries: &[HistoryEntry]) -> String {
    let header = "id,download_id,file_name,url,total_bytes,completed_at,duration_seconds,avg_speed,destination_path";
    let mut out = String::from(header);
    out.push_str("\r\n");
    for e in entries {
        let row = [
            e.id.to_string(),
            e.download_id.0.to_string(),
            escape_csv_field(&e.file_name),
            escape_csv_field(&e.url),
            e.total_bytes.to_string(),
            e.completed_at.to_string(),
            e.duration_seconds.to_string(),
            e.avg_speed.to_string(),
            escape_csv_field(&e.destination_path),
        ]
        .join(",");
        out.push_str(&row);
        out.push_str("\r\n");
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJsonEntry<'a> {
    id: u64,
    download_id: u64,
    file_name: &'a str,
    url: &'a str,
    total_bytes: u64,
    completed_at: u64,
    duration_seconds: u64,
    avg_speed: u64,
    destination_path: &'a str,
}

fn encode_json(entries: &[HistoryEntry]) -> Result<String, AppError> {
    let rows: Vec<HistoryJsonEntry<'_>> = entries
        .iter()
        .map(|e| HistoryJsonEntry {
            id: e.id,
            download_id: e.download_id.0,
            file_name: &e.file_name,
            url: &e.url,
            total_bytes: e.total_bytes,
            completed_at: e.completed_at,
            duration_seconds: e.duration_seconds,
            avg_speed: e.avg_speed,
            destination_path: &e.destination_path,
        })
        .collect();
    serde_json::to_string_pretty(&rows).map_err(|e| AppError::Storage(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;

    fn entry(id: u64, name: &str, url: &str) -> HistoryEntry {
        HistoryEntry {
            id,
            download_id: DownloadId(id),
            file_name: name.to_string(),
            url: url.to_string(),
            total_bytes: 1024,
            completed_at: 1_700_000_000 + id,
            duration_seconds: 60,
            avg_speed: 17,
            destination_path: format!("/tmp/{name}"),
        }
    }

    #[test]
    fn test_csv_header_uses_crlf() {
        let encoded = encode_csv(&[]);
        assert!(encoded.starts_with(
            "id,download_id,file_name,url,total_bytes,completed_at,duration_seconds,avg_speed,destination_path\r\n"
        ));
    }

    #[test]
    fn test_csv_escapes_comma_and_quote_per_rfc_4180() {
        let mut e = entry(1, "movie, special.mkv", "https://ex.com/a\"b");
        e.destination_path = "/data/with, comma".to_string();
        let encoded = encode_csv(&[e]);
        assert!(encoded.contains("\"movie, special.mkv\""));
        assert!(encoded.contains("\"https://ex.com/a\"\"b\""));
        assert!(encoded.contains("\"/data/with, comma\""));
    }

    #[test]
    fn test_csv_plain_value_not_quoted() {
        let encoded = encode_csv(&[entry(1, "plain.bin", "https://ex.com/p")]);
        assert!(encoded.contains(",plain.bin,"));
        assert!(!encoded.contains("\"plain.bin\""));
    }

    #[test]
    fn test_csv_escapes_embedded_newline() {
        let mut e = entry(1, "multi\nline.txt", "https://ex.com/m");
        e.destination_path = "/tmp/multi\nline.txt".to_string();
        let encoded = encode_csv(&[e]);
        assert!(encoded.contains("\"multi\nline.txt\""));
    }

    #[test]
    fn test_csv_guards_formula_prefixes_from_injection() {
        for dangerous in [
            "=cmd|'/c calc'!A0",
            "+1+2",
            "-HYPERLINK(\"evil\")",
            "@SUM(1+1)",
        ] {
            let escaped = escape_csv_field(dangerous);
            assert!(
                escaped.starts_with('\'') || escaped.starts_with("\"'"),
                "formula-prefix value {dangerous:?} must be guarded, got {escaped:?}"
            );
        }
    }

    #[test]
    fn test_csv_leaves_safe_prefixes_untouched() {
        for safe in ["plain.bin", "https://ex.com/x", "123abc"] {
            let escaped = escape_csv_field(safe);
            assert!(
                !escaped.starts_with('\''),
                "safe value {safe:?} should not be guarded, got {escaped:?}"
            );
        }
    }

    #[test]
    fn test_json_pretty_prints_entries() {
        let encoded = encode_json(&[entry(1, "alpha.zip", "https://ex.com/a")]).unwrap();
        // Pretty format includes newlines and 2-space indentation
        assert!(encoded.contains("\n  {"));
        assert!(encoded.contains("\"fileName\": \"alpha.zip\""));
        assert!(encoded.contains("\"downloadId\": 1"));
    }

    #[test]
    fn test_json_empty_returns_empty_array() {
        let encoded = encode_json(&[]).unwrap();
        assert_eq!(encoded, "[]");
    }

    #[tokio::test]
    async fn handle_export_history_writes_csv_file() {
        use std::sync::Arc;

        use crate::application::test_support::{InMemoryHistoryRepo, make_history_command_bus};
        use crate::domain::ports::driven::HistoryRepository;

        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&entry(1, "movie, one.mkv", "https://ex.com/a"))
            .unwrap();
        repo.record(&entry(2, "two.zip", "https://ex.com/b"))
            .unwrap();
        let bus = make_history_command_bus(repo.clone());

        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("history.csv");
        let count = bus
            .handle_export_history(ExportHistoryCommand {
                format: ExportHistoryFormat::Csv,
                path: out.clone(),
            })
            .await
            .unwrap();
        assert_eq!(count, 2);

        let contents = std::fs::read_to_string(&out).unwrap();
        assert!(contents.contains("\"movie, one.mkv\""));
        assert!(contents.contains("two.zip"));
    }

    #[tokio::test]
    async fn handle_export_history_pages_past_server_cap() {
        use std::sync::Arc;

        use crate::application::test_support::{InMemoryHistoryRepo, make_history_command_bus};
        use crate::domain::ports::driven::HistoryRepository;

        // Exceed MAX_HISTORY_PAGE_SIZE (500) so the handler has to paginate
        // through multiple list() calls instead of silently truncating.
        let repo = Arc::new(InMemoryHistoryRepo::new());
        for i in 1..=620u64 {
            repo.record(&entry(
                i,
                &format!("f{i}.bin"),
                &format!("https://ex.com/{i}"),
            ))
            .unwrap();
        }
        let bus = make_history_command_bus(repo);

        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("history.json");
        let count = bus
            .handle_export_history(ExportHistoryCommand {
                format: ExportHistoryFormat::Json,
                path: out.clone(),
            })
            .await
            .unwrap();
        assert_eq!(count, 620);

        let contents = std::fs::read_to_string(&out).unwrap();
        let value: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 620);
    }

    #[tokio::test]
    async fn handle_export_history_writes_json_file() {
        use std::sync::Arc;

        use crate::application::test_support::{InMemoryHistoryRepo, make_history_command_bus};
        use crate::domain::ports::driven::HistoryRepository;

        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&entry(1, "one.zip", "https://ex.com/a"))
            .unwrap();
        let bus = make_history_command_bus(repo);

        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("history.json");
        bus.handle_export_history(ExportHistoryCommand {
            format: ExportHistoryFormat::Json,
            path: out.clone(),
        })
        .await
        .unwrap();

        let contents = std::fs::read_to_string(&out).unwrap();
        let value: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["fileName"], "one.zip");
    }
}
