//! Running a child process with a timeout, as `execFile`/`execFileSync` do.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct RunOptions {
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
    pub env: Vec<(String, String)>,
    /// Keep stderr (for error messages) rather than discarding it.
    pub capture_stderr: bool,
}

pub struct RunOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

/// Runs `program args...`, killing it after `timeout`. Never opens a console window on Windows.
pub fn run_with_timeout(program: &str, args: &[&str], opts: &RunOptions) -> std::io::Result<RunOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped());
    cmd.stderr(if opts.capture_stderr { Stdio::piped() } else { Stdio::null() });
    if let Some(cwd) = &opts.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn()?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = stdout_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = stderr_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if !opts.timeout.is_zero() && start.elapsed() >= opts.timeout {
            let _ = child.kill();
            timed_out = true;
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    Ok(RunOutput { success: status.success() && !timed_out, code: status.code(), stdout, stderr, timed_out })
}
