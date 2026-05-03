use criterion::{Criterion, black_box, criterion_group, criterion_main};
use vortex_lib::domain::model::{
    AppConfig, ChecksumAlgorithm, ConfigPatch, Download, DownloadId, Priority, Segment, Url,
};
use vortex_lib::domain::model::config::{
    apply_patch, normalize_link_check_parallelism, normalize_max_concurrent,
};
use vortex_lib::domain::model::link::LinkStatus;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_download() -> Download {
    let url = Url::new("https://cdn.example.com/releases/v2.0/archive.tar.gz").unwrap();
    Download::new(DownloadId(1), url, "archive.tar.gz".to_string(), "/tmp".to_string())
}

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

fn bench_url_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_parsing");

    group.bench_function("simple_https", |b| {
        b.iter(|| Url::new(black_box("https://example.com/file.zip")))
    });

    group.bench_function("complex_with_port_and_path", |b| {
        b.iter(|| {
            Url::new(black_box(
                "https://cdn.example.com:8443/releases/v2.0/archive.tar.gz?token=abc123#section",
            ))
        })
    });

    group.bench_function("with_userinfo", |b| {
        b.iter(|| Url::new(black_box("https://user:pass@cdn.example.com/file.bin")))
    });

    group.bench_function("ftp_scheme", |b| {
        b.iter(|| Url::new(black_box("ftp://mirror.example.org/pub/release.iso")))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Download state machine
// ---------------------------------------------------------------------------

fn bench_download_state_machine(c: &mut Criterion) {
    let mut group = c.benchmark_group("download_state_machine");

    group.bench_function("create_new", |b| {
        b.iter(|| {
            let url = Url::new("https://example.com/file.zip").unwrap();
            black_box(Download::new(
                DownloadId(1),
                url,
                "file.zip".to_string(),
                "/tmp".to_string(),
            ))
        })
    });

    group.bench_function("full_lifecycle", |b| {
        b.iter(|| {
            let mut d = make_download();
            let _ = d.start();
            let _ = d.pause();
            let _ = d.resume();
            let _ = d.complete();
            black_box(&d);
        })
    });

    group.bench_function("start_and_fail_retry_cycle", |b| {
        b.iter(|| {
            let mut d = make_download().with_max_retries(3);
            let _ = d.start();
            let _ = d.fail("network timeout".to_string());
            let _ = d.retry();
            let _ = d.start();
            let _ = d.fail("connection reset".to_string());
            let _ = d.retry();
            black_box(&d);
        })
    });

    group.bench_function("progress_percentage", |b| {
        let mut d = make_download();
        d.set_file_size(10_000_000);
        d.update_progress(7_500_000);
        b.iter(|| black_box(d.progress_percentage()))
    });

    group.bench_function("checksum_workflow", |b| {
        b.iter(|| {
            let mut d = make_download()
                .with_expected_checksum(
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                )
                .unwrap();
            let _ = d.start();
            let _ = d.start_checking();
            let _ = d.record_checksum_match(
                ChecksumAlgorithm::Sha256,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            );
            black_box(&d);
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Segment operations
// ---------------------------------------------------------------------------

fn bench_segment_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_operations");

    group.bench_function("create_new", |b| {
        b.iter(|| black_box(Segment::new(1, DownloadId(10), 0, 10_485_760)))
    });

    group.bench_function("full_lifecycle", |b| {
        b.iter(|| {
            let mut s = Segment::new(1, DownloadId(10), 0, 10_485_760);
            let _ = s.start();
            s.update_progress(5_000_000);
            let _ = s.complete();
            black_box(&s);
        })
    });

    group.bench_function("split", |b| {
        b.iter(|| {
            let mut s = Segment::new(1, DownloadId(10), 0, 10_485_760);
            let _ = s.start();
            s.update_progress(2_000_000);
            let _ = s.split(5_242_880, 42);
            black_box(&s);
        })
    });

    group.bench_function("error_and_reset_cycle", |b| {
        b.iter(|| {
            let mut s = Segment::new(1, DownloadId(10), 0, 10_485_760);
            let _ = s.start();
            s.update_progress(1_000_000);
            let _ = s.fail("timeout".to_string());
            let _ = s.reset();
            let _ = s.start();
            let _ = s.complete();
            black_box(&s);
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Checksum detection
// ---------------------------------------------------------------------------

fn bench_checksum_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("checksum_detection");

    group.bench_function("detect_sha256", |b| {
        b.iter(|| {
            ChecksumAlgorithm::detect_from_hex(black_box(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ))
        })
    });

    group.bench_function("detect_md5", |b| {
        b.iter(|| {
            ChecksumAlgorithm::detect_from_hex(black_box("d41d8cd98f00b204e9800998ecf8427e"))
        })
    });

    group.bench_function("reject_invalid", |b| {
        b.iter(|| ChecksumAlgorithm::detect_from_hex(black_box("not-a-valid-hex-string")))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Link status
// ---------------------------------------------------------------------------

fn bench_link_status(c: &mut Criterion) {
    let mut group = c.benchmark_group("link_status");

    group.bench_function("from_status_code_200", |b| {
        b.iter(|| LinkStatus::from_status_code(black_box(200)))
    });

    group.bench_function("from_status_code_404", |b| {
        b.iter(|| LinkStatus::from_status_code(black_box(404)))
    });

    group.bench_function("from_status_code_500", |b| {
        b.iter(|| LinkStatus::from_status_code(black_box(500)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Config operations
// ---------------------------------------------------------------------------

fn bench_config_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_operations");

    group.bench_function("default_config", |b| {
        b.iter(|| black_box(AppConfig::default()))
    });

    group.bench_function("apply_patch_single_field", |b| {
        let patch = ConfigPatch {
            max_concurrent_downloads: Some(8),
            ..Default::default()
        };
        b.iter(|| {
            let mut config = AppConfig::default();
            apply_patch(&mut config, black_box(&patch));
            black_box(&config);
        })
    });

    group.bench_function("apply_patch_many_fields", |b| {
        let patch = ConfigPatch {
            max_concurrent_downloads: Some(8),
            max_segments_per_download: Some(16),
            speed_limit_bytes_per_sec: Some(Some(10_485_760)),
            max_retries: Some(10),
            theme: Some("dark".to_string()),
            locale: Some("fr".to_string()),
            dynamic_split_enabled: Some(true),
            verify_checksums: Some(true),
            link_check_parallelism: Some(16),
            link_check_timeout_secs: Some(30),
            ..Default::default()
        };
        b.iter(|| {
            let mut config = AppConfig::default();
            apply_patch(&mut config, black_box(&patch));
            black_box(&config);
        })
    });

    group.bench_function("normalize_max_concurrent", |b| {
        b.iter(|| black_box(normalize_max_concurrent(black_box(10))))
    });

    group.bench_function("normalize_link_check_parallelism", |b| {
        b.iter(|| black_box(normalize_link_check_parallelism(black_box(32))))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

fn bench_priority(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority");

    group.bench_function("create_valid", |b| {
        b.iter(|| black_box(Priority::new(black_box(5))))
    });

    group.bench_function("create_invalid", |b| {
        b.iter(|| black_box(Priority::new(black_box(0))))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_url_parsing,
    bench_download_state_machine,
    bench_segment_operations,
    bench_checksum_detection,
    bench_link_status,
    bench_config_operations,
    bench_priority,
);
criterion_main!(benches);
