use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use command_group::{CommandGroup, GroupChild};

#[cfg(test)]
use super::DEFAULT_OUTPUT_LIMIT;
use super::output::{self, ReaderHandle};
use super::platform;
use super::validation;
use super::{PreparedCommand, YtDlpResponse};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(super) fn execute(prepared: PreparedCommand) -> anyhow::Result<YtDlpResponse> {
    execute_with_discovery(prepared, platform::discover_ytdlp)
}

fn execute_with_discovery(
    prepared: PreparedCommand,
    discover: impl FnOnce() -> anyhow::Result<std::path::PathBuf>,
) -> anyhow::Result<YtDlpResponse> {
    let result = (|| {
        std::fs::create_dir_all(&prepared.working_dir).with_context(|| {
            format!(
                "run_ytdlp: failed to create working directory '{}'",
                prepared.working_dir.display()
            )
        })?;
        run_process_capped(
            &discover()?,
            &prepared.args,
            &prepared.working_dir,
            prepared.timeout,
            prepared.stdout_limit,
            prepared.stderr_limit,
        )
    })();

    let failed = result
        .as_ref()
        .map_or(true, |response| response.exit_code != 0);
    if prepared.cleanup_working_dir_on_failure
        && failed
        && let Err(error) = validation::cleanup_private_job_dir(&prepared.working_dir)
    {
        tracing::warn!(
            path = %prepared.working_dir.display(),
            error = %error,
            "failed to clean unsuccessful yt-dlp job directory"
        );
    }
    result
}

#[cfg(test)]
pub(super) fn execute_with_test_discovery(
    prepared: PreparedCommand,
    discover: impl FnOnce() -> anyhow::Result<std::path::PathBuf>,
) -> anyhow::Result<YtDlpResponse> {
    execute_with_discovery(prepared, discover)
}

#[cfg(test)]
pub(super) fn run_process(
    binary: &Path,
    args: &[String],
    working_dir: &Path,
    timeout: Duration,
) -> anyhow::Result<YtDlpResponse> {
    run_process_capped(
        binary,
        args,
        working_dir,
        timeout,
        DEFAULT_OUTPUT_LIMIT,
        DEFAULT_OUTPUT_LIMIT,
    )
}

fn run_process_capped(
    binary: &Path,
    args: &[String],
    working_dir: &Path,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> anyhow::Result<YtDlpResponse> {
    let mut child = spawn_group(binary, args, working_dir)?;
    let stdout = output::spawn_reader(child.inner().stdout.take(), stdout_limit);
    let stderr = output::spawn_reader(child.inner().stderr.take(), stderr_limit);
    let status = wait_for_group(&mut child, &stdout, &stderr, timeout);
    let output = output::collect(stdout, stderr, stdout_limit, stderr_limit);
    let status = status?;
    let (stdout, stderr) = output?;
    Ok(YtDlpResponse {
        exit_code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

fn spawn_group(binary: &Path, args: &[String], cwd: &Path) -> anyhow::Result<GroupChild> {
    if !binary.is_absolute() {
        bail!("run_ytdlp: approved binary path must be absolute");
    }
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", platform::controlled_path(binary)?)
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    platform::copy_required_environment(&mut command);
    command
        .group_spawn()
        .with_context(|| format!("run_ytdlp: failed to spawn '{}'", binary.display()))
}

fn wait_for_group(
    child: &mut GroupChild,
    stdout: &ReaderHandle,
    stderr: &ReaderHandle,
    timeout: Duration,
) -> anyhow::Result<ExitStatus> {
    let started = Instant::now();
    let mut status = None;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(result) => status = result,
                Err(error) => {
                    terminate_group(child)
                        .context("run_ytdlp: failed to clean up process after poll error")?;
                    return Err(error).context("run_ytdlp: failed to poll process group");
                }
            }
        }
        if let Some(status) = status
            && stdout.is_finished()
            && stderr.is_finished()
        {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            terminate_group(child)?;
            bail!(
                "run_ytdlp: process timed out after {}ms",
                timeout.as_millis()
            );
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn terminate_group(child: &mut GroupChild) -> anyhow::Result<()> {
    let kill_error = child.kill().err();
    child
        .wait()
        .context("run_ytdlp: failed to reap process group")?;
    if let Some(error) = kill_error
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        return Err(error).context("run_ytdlp: failed to kill process group");
    }
    Ok(())
}
