use std::path::PathBuf;
use std::time::{Duration, Instant};

use base64::Engine;

use super::tesseract_broker::{
    PluginTesseractRequest, run_with_discovery, run_with_discovery_timeout,
};
use crate::domain::model::captcha::MAX_CAPTCHA_IMAGE_BYTES;

fn png_image() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec()
}

fn request() -> PluginTesseractRequest {
    serde_json::from_value(serde_json::json!({
        "image_data": base64::engine::general_purpose::STANDARD.encode(png_image())
    }))
    .expect("valid request")
}

#[test]
fn tesseract_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<PluginTesseractRequest>(serde_json::json!({
            "image_data": "abc",
            "args": ["--arbitrary"]
        }))
        .is_err()
    );
}

#[test]
fn missing_tesseract_is_reported_as_unavailable() {
    let response = run_with_discovery("vortex-mod-captcha-ocr", request(), || Ok(None))
        .expect("missing binary is not a broker failure");

    assert_eq!(response.status, "unavailable");
    assert!(response.solution.is_none());
}

#[cfg(unix)]
#[test]
fn tesseract_receives_image_on_stdin_and_only_fixed_arguments() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let binary = temp.path().join("tesseract");
    std::fs::write(
        &binary,
        "#!/bin/sh\n[ \"$1\" = stdin ] && [ \"$2\" = stdout ] && [ \"$3\" = -l ] && [ \"$4\" = eng ] && [ \"$5\" = --psm ] && [ \"$6\" = 7 ] || exit 9\n/bin/cat >/dev/null\nprintf ' ABC123 \\n'\n",
    )
    .expect("write fake tesseract");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
        .expect("make executable");

    let response = run_with_discovery("vortex-mod-captcha-ocr", request(), || {
        Ok(Some(PathBuf::from(&binary)))
    })
    .expect("run fake tesseract");

    assert_eq!(response.status, "solved");
    assert_eq!(response.solution.as_deref(), Some("ABC123"));
}

#[test]
fn tesseract_broker_rejects_other_plugins() {
    let error = run_with_discovery("untrusted-plugin", request(), || Ok(None))
        .expect_err("broker must be scoped to the OCR plugin");

    assert!(error.to_string().contains("not authorized"));
}

#[cfg(unix)]
#[test]
fn timeout_also_bounds_a_child_that_never_reads_stdin() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let binary = temp.path().join("tesseract");
    std::fs::write(&binary, "#!/bin/sh\nsleep 60\n").expect("write fake tesseract");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
        .expect("make executable");
    let mut image = png_image();
    image.resize(MAX_CAPTCHA_IMAGE_BYTES, 0);
    let request = serde_json::from_value(serde_json::json!({
        "image_data": base64::engine::general_purpose::STANDARD.encode(image)
    }))
    .expect("valid request");

    let started = Instant::now();
    let error = run_with_discovery_timeout(
        "vortex-mod-captcha-ocr",
        request,
        || Ok(Some(PathBuf::from(&binary))),
        Duration::from_millis(100),
    )
    .expect_err("blocked stdin must time out");

    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(3));
}
