use criterion::{Criterion, black_box, criterion_group, criterion_main};

use vortex_lib::domain::model::config::{apply_patch, normalize_max_concurrent};
use vortex_lib::domain::model::{
    AppConfig, ChecksumAlgorithm, ConfigPatch, Download, DownloadId, DownloadState, Priority,
    Segment, SegmentState, Url,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_url() -> Url {
    Url::new("https://cdn.example.com/releases/v2.0/archive.tar.gz").unwrap()
}

fn make_download() -> Download {
    Download::new(
        DownloadId(1),
        make_url(),
        "archive.tar.gz".to_string(),
        "/tmp/downloads".to_string(),
    )
}

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

fn bench_url_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parsing");

    group.bench_function("parse_https_simple", |b| {
        b.iter(|| Url::new(black_box("https://example.com/file.zip")))
    });

    group.bench_function("parse_https_complex", |b| {
        b.iter(|| {
            Url::new(black_box(
                "https://user:pass@cdn.example.com:8443/path/to/file.tar.gz?token=abc123#section",
            ))
        })
    });

    group.bench_function("parse_ftp", |b| {
        b.iter(|| {
            Url::new(black_box(
                "ftp://mirror.example.org/pub/releases/latest.iso",
            ))
        })
    });

    group.bench_function("parse_invalid_scheme", |b| {
        b.iter(|| Url::new(black_box("ssh://example.com/file")))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Download state machine
// ---------------------------------------------------------------------------

fn bench_download_state_machine(c: &mut Criterion) {
    let mut group = c.benchmark_group("download_state_machine");

    group.bench_function("create", |b| {
        b.iter(|| {
            let url = Url::new("https://example.com/file.zip").unwrap();
            Download::new(
                DownloadId(black_box(1)),
                url,
                "file.zip".to_string(),
                "/tmp".to_string(),
            )
        })
    });

    group.bench_function("start_from_queued", |b| {
        b.iter_batched(
            make_download,
            |mut d| d.start(),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("full_lifecycle", |b| {
        b.iter_batched(
            make_download,
            |mut d| {
                d.start().unwrap();
                d.pause().unwrap();
                d.resume().unwrap();
                d.complete().unwrap();
                d
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("retry_cycle", |b| {
        b.iter_batched(
            || {
                let mut d = make_download().with_max_retries(5);
                d.start().unwrap();
                d.fail("network error".to_string()).unwrap();
                d
            },
            |mut d| {
                d.retry().unwrap();
                d.start().unwrap();
                d
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("progress_percentage", |b| {
        let mut d = make_download();
        d.set_file_size(10_000_000);
        d.update_progress(7_500_000);
        b.iter(|| black_box(&d).progress_percentage())
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Segment operations
// ---------------------------------------------------------------------------

fn bench_segment_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_operations");

    group.bench_function("create", |b| {
        b.iter(|| Segment::new(black_box(1), DownloadId(10), 0, 1_048_576))
    });

    group.bench_function("lifecycle", |b| {
        b.iter_batched(
            || Segment::new(1, DownloadId(10), 0, 1_048_576),
            |mut s| {
                s.start().unwrap();
                s.update_progress(524_288);
                s.complete().unwrap();
                s
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("split", |b| {
        b.iter_batched(
            || {
                let mut s = Segment::new(1, DownloadId(10), 0, 1_000_000);
                s.start().unwrap();
                s.update_progress(200_000);
                s
            },
            |mut s| s.split(600_000, 42),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("error_and_reset", |b| {
        b.iter_batched(
            || {
                let mut s = Segment::new(1, DownloadId(10), 0, 1_048_576);
                s.start().unwrap();
                s.fail("timeout".to_string()).unwrap();
                s
            },
            |mut s| s.reset(),
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Checksum detection
// ---------------------------------------------------------------------------

fn bench_checksum_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("checksum_detection");

    group.bench_function("detect_sha256", |b| {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        b.iter(|| ChecksumAlgorithm::detect_from_hex(black_box(hash)))
    });

    group.bench_function("detect_md5", |b| {
        let hash = "d41d8cd98f00b204e9800998ecf8427e";
        b.iter(|| ChecksumAlgorithm::detect_from_hex(black_box(hash)))
    });

    group.bench_function("detect_unsupported_length", |b| {
        let hash = "a".repeat(40); // SHA-1 length, not supported
        b.iter(|| ChecksumAlgorithm::detect_from_hex(black_box(&hash)))
    });

    group.bench_function("detect_non_hex", |b| {
        let hash = "z".repeat(64);
        b.iter(|| ChecksumAlgorithm::detect_from_hex(black_box(&hash)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Config patch application
// ---------------------------------------------------------------------------

fn bench_config_patch(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_patch");

    group.bench_function("apply_empty_patch", |b| {
        let patch = ConfigPatch::default();
        b.iter_batched(
            AppConfig::default,
            |mut config| apply_patch(&mut config, black_box(&patch)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("apply_full_patch", |b| {
        let patch = ConfigPatch {
            download_dir: Some(Some("/home/user/Downloads".to_string())),
            max_concurrent_downloads: Some(8),
            max_segments_per_download: Some(16),
            speed_limit_bytes_per_sec: Some(Some(1_048_576)),
            max_retries: Some(10),
            verify_checksums: Some(true),
            dynamic_split_enabled: Some(false),
            dynamic_split_min_remaining_mb: Some(8),
            history_retention_days: Some(90),
            proxy_type: Some("http".to_string()),
            proxy_url: Some(Some("http://proxy:8080".to_string())),
            theme: Some("dark".to_string()),
            link_check_parallelism: Some(16),
            link_check_timeout_secs: Some(30),
            ..Default::default()
        };
        b.iter_batched(
            AppConfig::default,
            |mut config| apply_patch(&mut config, black_box(&patch)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("normalize_max_concurrent", |b| {
        b.iter(|| normalize_max_concurrent(black_box(4)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

fn bench_priority(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority");

    group.bench_function("create_valid", |b| b.iter(|| Priority::new(black_box(5))));

    group.bench_function("create_invalid", |b| b.iter(|| Priority::new(black_box(0))));

    group.bench_function("compare", |b| {
        let low = Priority::new(1).unwrap();
        let high = Priority::new(10).unwrap();
        b.iter(|| black_box(&low) < black_box(&high))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Download state parsing
// ---------------------------------------------------------------------------

fn bench_download_state_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("download_state_parsing");

    group.bench_function("parse_valid_state", |b| {
        b.iter(|| black_box("Downloading").parse::<DownloadState>())
    });

    group.bench_function("parse_invalid_state", |b| {
        b.iter(|| black_box("InvalidState").parse::<DownloadState>())
    });

    group.bench_function("display_state", |b| {
        let state = DownloadState::Downloading;
        b.iter(|| format!("{}", black_box(state)))
    });

    group.bench_function("parse_all_states", |b| {
        let states = [
            "Queued",
            "Downloading",
            "Paused",
            "Waiting",
            "Retry",
            "Error",
            "Extracting",
            "Completed",
            "Checking",
        ];
        b.iter(|| {
            for s in &states {
                let _ = black_box(s).parse::<DownloadState>();
            }
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// SegmentState parsing
// ---------------------------------------------------------------------------

fn bench_segment_state_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_state_parsing");

    group.bench_function("parse_valid", |b| {
        b.iter(|| black_box("Downloading").parse::<SegmentState>())
    });

    group.bench_function("display", |b| {
        let state = SegmentState::Completed;
        b.iter(|| format!("{}", black_box(state)))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_url_parsing,
    bench_download_state_machine,
    bench_segment_operations,
    bench_checksum_detection,
    bench_config_patch,
    bench_priority,
    bench_download_state_parsing,
    bench_segment_state_parsing,
);
criterion_main!(benches);
