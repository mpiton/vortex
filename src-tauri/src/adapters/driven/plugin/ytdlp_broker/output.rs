use std::io::Read;
use std::thread::JoinHandle;

use anyhow::bail;

pub(super) struct CapturedOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

pub(super) type ReaderHandle = JoinHandle<std::io::Result<CapturedOutput>>;

pub(super) fn read_stream_capped<R: Read>(
    mut reader: R,
    max_bytes: usize,
) -> std::io::Result<CapturedOutput> {
    let mut output = CapturedOutput {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(output);
        }
        let copied = max_bytes.saturating_sub(output.bytes.len()).min(read);
        output.bytes.extend_from_slice(&chunk[..copied]);
        output.truncated |= copied < read;
    }
}

pub(super) fn spawn_reader<T>(stream: Option<T>, max_bytes: usize) -> ReaderHandle
where
    T: Read + Send + 'static,
{
    std::thread::spawn(move || match stream {
        Some(stream) => read_stream_capped(stream, max_bytes),
        None => Ok(CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        }),
    })
}

pub(super) fn collect(
    stdout: ReaderHandle,
    stderr: ReaderHandle,
    stdout_limit: usize,
    stderr_limit: usize,
) -> anyhow::Result<(String, String)> {
    let stdout = stdout.join();
    let stderr = stderr.join();
    let stdout =
        stdout.map_err(|_| anyhow::anyhow!("run_ytdlp: stdout reader thread panicked"))??;
    let stderr =
        stderr.map_err(|_| anyhow::anyhow!("run_ytdlp: stderr reader thread panicked"))??;
    decode(
        "stdout",
        stdout,
        stdout_limit,
        "stderr",
        stderr,
        stderr_limit,
    )
}

fn decode(
    first_name: &str,
    first: CapturedOutput,
    first_limit: usize,
    second_name: &str,
    second: CapturedOutput,
    second_limit: usize,
) -> anyhow::Result<(String, String)> {
    if first.truncated || second.truncated {
        let (stream, limit) = if first.truncated {
            (first_name, first_limit)
        } else {
            (second_name, second_limit)
        };
        bail!("run_ytdlp: {stream} exceeded the {limit}-byte limit");
    }
    Ok((
        String::from_utf8_lossy(&first.bytes).into_owned(),
        String::from_utf8_lossy(&second.bytes).into_owned(),
    ))
}
