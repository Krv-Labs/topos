//! Minimal subprocess-with-timeout helper shared by [`super::discovery`]'s
//! `git` calls and [`super::gitnexus`]'s `gitnexus analyze` invocation.
//!
//! # Deviation from the Python original
//! Both Python callers rely on `subprocess.run(..., timeout=...)`, which
//! the CPython runtime implements natively. `std::process::Command` has no
//! equivalent — Rust's stdlib can spawn and wait, but cannot wait *with a
//! deadline*. Rather than pull in a dependency (`wait-timeout`, `tokio`,
//! ...) for one small feature, this is a manual poll-and-kill loop: spawn,
//! then poll `try_wait` on a short interval until the process exits or the
//! deadline passes, killing it on the latter. Captured stdout/stderr are
//! drained on background threads *while* we poll, so a chatty child can't
//! deadlock us against a full pipe buffer.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Whether `name` resolves to an executable file on `$PATH` — a small
/// stand-in for `shutil.which`, shared by every adapter that needs to check
/// an external tool's availability before shelling out to it (`gitnexus`,
/// `graphify`, ...) rather than pulling in a `which` crate dependency for
/// semantics (Windows `PATHEXT`, symlink resolution, ...) none of them need.
pub(crate) fn command_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| is_executable_file(&dir.join(name)))
}

/// Poll interval for the `try_wait` loop below.
///
/// ponytail: fixed poll interval, not exponential backoff — every caller's
/// timeout is on the order of seconds to minutes, so a flat 20ms adds
/// negligible latency without needing tuning.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Captured (or empty, if not captured) output of a finished process.
#[derive(Debug, Clone, Default)]
pub struct RunOutput {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Why [`run_with_timeout`] didn't return a [`RunOutput`].
#[derive(Debug)]
pub enum RunError {
    /// The process outlived `timeout` and was killed.
    TimedOut,
    /// The command could not be spawned, or waiting on it failed.
    Io(std::io::Error),
}

/// Run `cmd` in `cwd`, capturing stdout/stderr when `capture` is set
/// (otherwise they're inherited from this process, for a live CLI stream),
/// killing it if it outlives `timeout` (`None` disables the deadline).
pub fn run_with_timeout(
    mut cmd: Command,
    cwd: Option<&Path>,
    capture: bool,
    timeout: Option<Duration>,
) -> Result<RunOutput, RunError> {
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

    let mut child = cmd.spawn().map_err(RunError::Io)?;
    let stdout_reader = capture.then(|| spawn_reader(child.stdout.take()));
    let stderr_reader = capture.then(|| spawn_reader(child.stderr.take()));

    let start = Instant::now();
    loop {
        match child.try_wait().map_err(RunError::Io)? {
            Some(status) => {
                return Ok(RunOutput {
                    status_code: status.code(),
                    stdout: stdout_reader.map(join_reader).unwrap_or_default(),
                    stderr: stderr_reader.map(join_reader).unwrap_or_default(),
                });
            }
            None => {
                if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                    let _ = child.kill();
                    let _ = child.wait();
                    stdout_reader.map(join_reader);
                    stderr_reader.map(join_reader);
                    return Err(RunError::TimedOut);
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_string(&mut buf);
        }
        buf
    })
}

fn join_reader(handle: std::thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_of_a_quick_command() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let out = run_with_timeout(cmd, None, true, Some(Duration::from_secs(5))).unwrap();
        assert_eq!(out.status_code, Some(0));
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[test]
    fn kills_a_process_that_outlives_the_timeout() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let start = Instant::now();
        let result = run_with_timeout(cmd, None, true, Some(Duration::from_millis(100)));
        assert!(matches!(result, Err(RunError::TimedOut)));
        // The kill actually happened promptly — this isn't just a `sleep 5`
        // that happened to return an unrelated error quickly.
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn nonexistent_command_is_an_io_error() {
        let cmd = Command::new("topos-adapters-nonexistent-binary-xyz");
        let result = run_with_timeout(cmd, None, true, None);
        assert!(matches!(result, Err(RunError::Io(_))));
    }
}
