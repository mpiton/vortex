use std::io::{Cursor, Seek};
use std::time::{Duration, Instant};

use super::super::output::{collect, read_stream_capped, spawn_reader};
use super::super::process::{execute_with_test_discovery, run_process};
use super::super::validation;
use super::super::{DEFAULT_OUTPUT_LIMIT, METADATA_OUTPUT_LIMIT, PreparedCommand};
use super::private_root;

#[test]
fn output_reader_caps_but_drains_the_stream() {
    let mut cursor = Cursor::new(vec![b'x'; 17]);
    let output = read_stream_capped(&mut cursor, 16).expect("read output");
    assert_eq!(output.bytes.len(), 16);
    assert!(output.truncated);
    assert_eq!(cursor.stream_position().unwrap(), 17);
}

#[test]
fn oversized_output_is_reported_instead_of_returning_truncated_data() {
    let stdout = spawn_reader(
        Some(Cursor::new(vec![b'x'; DEFAULT_OUTPUT_LIMIT + 1])),
        DEFAULT_OUTPUT_LIMIT,
    );
    let stderr = spawn_reader(Some(Cursor::new(Vec::new())), DEFAULT_OUTPUT_LIMIT);

    let error = collect(stdout, stderr, DEFAULT_OUTPUT_LIMIT, DEFAULT_OUTPUT_LIMIT)
        .expect_err("oversized output must fail");

    assert!(error.to_string().contains("stdout exceeded"));
}

#[test]
fn metadata_output_accepts_large_playlist_below_its_bounded_cap() {
    let stdout = spawn_reader(
        Some(Cursor::new(vec![b'x'; DEFAULT_OUTPUT_LIMIT + 1])),
        METADATA_OUTPUT_LIMIT,
    );
    let stderr = spawn_reader(Some(Cursor::new(Vec::new())), DEFAULT_OUTPUT_LIMIT);

    let (stdout, _) = collect(stdout, stderr, METADATA_OUTPUT_LIMIT, DEFAULT_OUTPUT_LIMIT)
        .expect("metadata has a larger bounded allowance");
    assert_eq!(stdout.len(), DEFAULT_OUTPUT_LIMIT + 1);
}

#[test]
fn metadata_output_still_rejects_data_above_its_cap() {
    let stdout = spawn_reader(
        Some(Cursor::new(vec![b'x'; METADATA_OUTPUT_LIMIT + 1])),
        METADATA_OUTPUT_LIMIT,
    );
    let stderr = spawn_reader(Some(Cursor::new(Vec::new())), DEFAULT_OUTPUT_LIMIT);

    collect(stdout, stderr, METADATA_OUTPUT_LIMIT, DEFAULT_OUTPUT_LIMIT)
        .expect_err("metadata output must remain bounded");
}

#[test]
fn discovery_failure_cleans_partial_download_directory() {
    let temp = tempfile::tempdir().unwrap();
    let request = private_root(temp.path());
    let job = validation::create_private_child(&request, "job-").unwrap();
    std::fs::write(job.join("partial.part"), b"partial").unwrap();
    let prepared = PreparedCommand {
        args: Vec::new(),
        working_dir: job.clone(),
        timeout: Duration::from_secs(1),
        stdout_limit: DEFAULT_OUTPUT_LIMIT,
        stderr_limit: DEFAULT_OUTPUT_LIMIT,
        cleanup_working_dir_on_failure: true,
    };

    execute_with_test_discovery(prepared, || Err(anyhow::anyhow!("yt-dlp is missing")))
        .expect_err("discovery must fail");

    assert!(!job.exists());
}

#[cfg(unix)]
#[test]
fn process_uses_controlled_environment_and_working_directory() {
    let temp = tempfile::tempdir().unwrap();
    let binary = fake_binary(temp.path(), "printf '%s\\n' \"${HOME-unset}\"\npwd\n");
    let response = run_process(&binary, &[], temp.path(), Duration::from_secs(1)).unwrap();
    let mut lines = response.stdout.lines();
    assert_eq!(lines.next(), Some("unset"));
    assert_eq!(lines.next(), temp.path().to_str());
}

#[cfg(unix)]
#[test]
fn timeout_kills_descendants_that_keep_output_pipes_open() {
    let temp = tempfile::tempdir().unwrap();
    let binary = fake_binary(temp.path(), "sleep 5 &\n");
    let started = Instant::now();
    let error = run_process(&binary, &[], temp.path(), Duration::from_millis(50))
        .expect_err("process group must time out");
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
fn fake_binary(directory: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let binary = directory.join("yt-dlp");
    std::fs::write(&binary, format!("#!/bin/sh\n{body}")).unwrap();
    let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&binary, permissions).unwrap();
    binary
}
