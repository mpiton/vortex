use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use base64::Engine;
use command_group::{CommandGroup, GroupChild};

use crate::domain::model::captcha::{
    MAX_CAPTCHA_IMAGE_BYTES, MAX_CAPTCHA_SOLUTION_BYTES, captcha_image_mime_type,
};

pub(crate) const OCR_PLUGIN_NAME: &str = "vortex-mod-captcha-ocr";
pub(crate) const MAX_TESSERACT_REQUEST_BYTES: usize = MAX_CAPTCHA_IMAGE_BYTES.div_ceil(3) * 4 + 64;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(15);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const OUTPUT_LIMIT: usize = MAX_CAPTCHA_SOLUTION_BYTES;
const ERROR_OUTPUT_LIMIT: usize = 4 * 1024;

pub(crate) fn supports_plugin(plugin_name: &str) -> bool {
    plugin_name == OCR_PLUGIN_NAME
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginTesseractRequest {
    image_data: String,
}

impl std::fmt::Debug for PluginTesseractRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginTesseractRequest")
            .field("image_data", &"<redacted>")
            .finish()
    }
}

#[derive(serde::Serialize)]
pub(crate) struct TesseractResponse {
    pub(crate) status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) solution: Option<String>,
}

impl std::fmt::Debug for TesseractResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TesseractResponse")
            .field("status", &self.status)
            .field("solution", &self.solution.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

pub(crate) fn run_plugin_request(
    plugin_name: &str,
    request: PluginTesseractRequest,
) -> anyhow::Result<TesseractResponse> {
    run_with_discovery(plugin_name, request, || Ok(discover_tesseract()))
}

pub(crate) fn run_with_discovery(
    plugin_name: &str,
    request: PluginTesseractRequest,
    discover: impl FnOnce() -> anyhow::Result<Option<PathBuf>>,
) -> anyhow::Result<TesseractResponse> {
    run_with_discovery_timeout(plugin_name, request, discover, PROCESS_TIMEOUT)
}

pub(crate) fn run_with_discovery_timeout(
    plugin_name: &str,
    request: PluginTesseractRequest,
    discover: impl FnOnce() -> anyhow::Result<Option<PathBuf>>,
    timeout: Duration,
) -> anyhow::Result<TesseractResponse> {
    if !supports_plugin(plugin_name) {
        bail!("run_tesseract: plugin is not authorized");
    }
    let image = decode_image(request)?;
    let Some(binary) = discover()? else {
        return Ok(TesseractResponse {
            status: "unavailable",
            solution: None,
        });
    };
    let output = execute(&binary, image, timeout)?;
    if !output.status.success() {
        return Ok(TesseractResponse {
            status: "rejected",
            solution: None,
        });
    }
    let solution = String::from_utf8(output.stdout)
        .context("run_tesseract: stdout is not valid UTF-8")?
        .trim()
        .to_string();
    if solution.is_empty() || solution.len() > MAX_CAPTCHA_SOLUTION_BYTES {
        return Ok(TesseractResponse {
            status: "rejected",
            solution: None,
        });
    }
    Ok(TesseractResponse {
        status: "solved",
        solution: Some(solution),
    })
}

fn decode_image(request: PluginTesseractRequest) -> anyhow::Result<Vec<u8>> {
    let max_encoded = MAX_CAPTCHA_IMAGE_BYTES.div_ceil(3) * 4;
    if request.image_data.len() > max_encoded {
        bail!("run_tesseract: encoded image exceeds safety limit");
    }
    let image = base64::engine::general_purpose::STANDARD
        .decode(request.image_data)
        .context("run_tesseract: image is not valid base64")?;
    if image.is_empty()
        || image.len() > MAX_CAPTCHA_IMAGE_BYTES
        || captcha_image_mime_type(&image).is_none()
    {
        bail!("run_tesseract: image is invalid or exceeds safety limits");
    }
    Ok(image)
}

fn discover_tesseract() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let mut roots = Vec::new();
    #[cfg(unix)]
    {
        candidates.extend([
            PathBuf::from("/opt/homebrew/bin/tesseract"),
            PathBuf::from("/usr/local/bin/tesseract"),
            PathBuf::from("/usr/bin/tesseract"),
            PathBuf::from("/run/current-system/sw/bin/tesseract"),
            PathBuf::from("/nix/var/nix/profiles/default/bin/tesseract"),
        ]);
        roots.extend([
            PathBuf::from("/opt/homebrew"),
            PathBuf::from("/usr/local"),
            PathBuf::from("/usr"),
            PathBuf::from("/run/current-system"),
            PathBuf::from("/nix/store"),
            PathBuf::from("/nix/var/nix/profiles"),
        ]);
    }
    #[cfg(windows)]
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let program_files = PathBuf::from(program_files);
        candidates.push(program_files.join("Tesseract-OCR/tesseract.exe"));
        roots.push(program_files);
    }
    super::ytdlp_broker::platform::find_approved_named_binary(
        &candidates,
        &roots,
        if cfg!(windows) {
            "tesseract.exe"
        } else {
            "tesseract"
        },
    )
    .ok()
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

fn execute(binary: &Path, image: Vec<u8>, timeout: Duration) -> anyhow::Result<ProcessOutput> {
    if !binary.is_absolute() {
        bail!("run_tesseract: approved binary path must be absolute");
    }
    let mut command = build_tesseract_command(binary);
    let mut child = command
        .group_spawn()
        .with_context(|| format!("run_tesseract: failed to spawn '{}'", binary.display()))?;

    let stdin = child
        .inner()
        .stdin
        .take()
        .context("run_tesseract: stdin is unavailable")?;
    let stdout = spawn_reader(child.inner().stdout.take(), OUTPUT_LIMIT);
    let stderr = spawn_reader(child.inner().stderr.take(), ERROR_OUTPUT_LIMIT);
    let stdin = spawn_writer(stdin, image);
    let status = wait_for_group(&mut child, timeout);
    let stdin = join_writer(stdin);
    let stdout = join_reader(stdout, "stdout");
    let stderr = join_reader(stderr, "stderr");
    let status = status?;
    stdin?;
    let stdout = stdout?;
    let _stderr = stderr?;
    Ok(ProcessOutput { status, stdout })
}

pub(crate) fn build_tesseract_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .args(["stdin", "stdout", "-l", "eng", "--psm", "7"])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    super::ytdlp_broker::platform::copy_required_environment(&mut command);
    command
}

fn spawn_writer(
    mut writer: impl Write + Send + 'static,
    image: Vec<u8>,
) -> std::thread::JoinHandle<anyhow::Result<()>> {
    std::thread::spawn(move || {
        writer
            .write_all(&image)
            .context("run_tesseract: failed to write image")
    })
}

fn join_writer(handle: std::thread::JoinHandle<anyhow::Result<()>>) -> anyhow::Result<()> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("run_tesseract: stdin writer panicked"))?
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: Option<R>,
    limit: usize,
) -> std::thread::JoinHandle<anyhow::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = reader.context("run_tesseract: process pipe is unavailable")?;
        reader
            .by_ref()
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .context("run_tesseract: failed to read process output")?;
        if bytes.len() > limit {
            bail!("run_tesseract: process output exceeds safety limit");
        }
        Ok(bytes)
    })
}

fn join_reader(
    handle: std::thread::JoinHandle<anyhow::Result<Vec<u8>>>,
    stream: &str,
) -> anyhow::Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("run_tesseract: {stream} reader panicked"))?
}

fn wait_for_group(child: &mut GroupChild, timeout: Duration) -> anyhow::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                terminate_group(child)?;
                return Err(error).context("run_tesseract: failed to poll process");
            }
        }
        if started.elapsed() >= timeout {
            terminate_group(child)?;
            bail!("run_tesseract: process timed out");
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn terminate_group(child: &mut GroupChild) -> anyhow::Result<()> {
    let kill_error = child.kill().err();
    child
        .wait()
        .context("run_tesseract: failed to reap process group")?;
    if let Some(error) = kill_error
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        return Err(error).context("run_tesseract: failed to kill process group");
    }
    Ok(())
}
