//! SEP-2663 background task lifecycle and subprocess cancellation regression tests.
//!
//! Exercises interactive `topos-mcp` sessions over stdio:
//! - Task creation (`resultType: "task"`), `tasks/get` polling, and completion output.
//! - Synchronous fast-path validation returning `resultType: "complete"` without
//!   allocating a background task.
//! - Inline fallback when the client does not declare `io.modelcontextprotocol/tasks`.
//! - Cooperative `tasks/cancel` and stdio EOF teardown killing in-flight `gitnexus`
//!   child process groups.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
mod libc {
    extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
}

/// Interactive line-delimited JSON-RPC session over `topos-mcp` stdio.
struct McpTestSession {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl McpTestSession {
    fn with_envs<K, V>(envs: &[(K, V)]) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_topos-mcp"));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn topos-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self {
            child,
            stdin: Some(stdin),
            stdout,
        }
    }

    fn send(&mut self, msg: &serde_json::Value) {
        let stdin = self
            .stdin
            .as_mut()
            .expect("stdin already closed on McpTestSession");
        let line = serde_json::to_string(msg).expect("serialize JSON-RPC message");
        writeln!(stdin, "{line}").expect("write JSON-RPC line");
        stdin.flush().expect("flush child stdin");
    }

    fn recv(&mut self) -> serde_json::Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .expect("read line from child stdout");
            assert!(n > 0, "unexpected EOF from topos-mcp stdout");
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("invalid JSON-RPC line ({e}): {line}"));
        }
    }

    fn wait(&mut self) -> ExitStatus {
        self.child.wait().expect("wait for topos-mcp exit")
    }
}

impl Drop for McpTestSession {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn meta_2026_with_tasks() -> serde_json::Value {
    serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {
            "extensions": {
                "io.modelcontextprotocol/tasks": {}
            }
        }
    })
}

fn meta_2026_no_tasks() -> serde_json::Value {
    serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// Create a minimal temporary project with `.git` and a single Rust source file.
fn make_temp_rust_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp project dir");
    std::fs::create_dir(dir.path().join(".git")).expect("create .git marker");
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .expect("write lib.rs");
    let canonical = dir.path().canonicalize().expect("canonicalize temp dir");
    (dir, canonical)
}

fn poll_until_status(
    session: &mut McpTestSession,
    task_id: &str,
    target_status: &str,
    timeout: Duration,
) -> serde_json::Value {
    let deadline = Instant::now() + timeout;
    let mut req_id = 100u64;
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for task {task_id} to reach {target_status}"
        );
        session.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "tasks/get",
            "params": {
                "taskId": task_id,
                "_meta": meta_2026_with_tasks()
            }
        }));
        req_id += 1;
        let poll_resp = session.recv();
        let poll_res = &poll_resp["result"];
        let status = poll_res["status"].as_str().unwrap_or_default();
        if status == target_status {
            return poll_res.clone();
        }
        assert_eq!(
            status, "working",
            "unexpected task status {status:?}: {poll_resp:#?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// SEP-2663 task creation, polling (`tasks/get`), and completion output on
/// `2026-07-28` when the client declares `io.modelcontextprotocol/tasks`.
#[test]
fn sep2663_task_lifecycle_create_poll_complete() {
    let (_project, project_path) = make_temp_rust_project();
    let mut session =
        McpTestSession::with_envs(&[("TOPOS_MCP_FILE_ROOT", project_path.as_os_str())]);

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_evaluate_project",
            "arguments": {
                "path": project_path.to_string_lossy(),
                "no_composable": true
            },
            "_meta": meta_2026_with_tasks()
        }
    }));

    let call_resp = session.recv();
    let call_res = &call_resp["result"];
    assert_eq!(
        call_res["resultType"], "task",
        "expected task response: {call_resp:#?}"
    );
    let task_id = call_res["taskId"]
        .as_str()
        .expect("taskId string")
        .to_string();
    assert!(!task_id.is_empty(), "taskId must not be empty");
    assert_eq!(call_res["ttlMs"], 600_000);
    assert_eq!(call_res["pollIntervalMs"], 1_000);

    let completed = poll_until_status(&mut session, &task_id, "completed", Duration::from_secs(10));
    let output = completed
        .get("result")
        .unwrap_or_else(|| panic!("completed task missing result: {completed:#?}"));
    assert_eq!(
        output["structuredContent"]["file_count"], 1,
        "unexpected completed task output: {output:#?}"
    );
}

/// Fast-path validation errors return `resultType: "complete"` immediately
/// without allocating a background task record.
#[test]
fn sep2663_fast_path_validation_returns_complete_without_spawning_task() {
    let (_project, project_path) = make_temp_rust_project();
    let missing_dir = project_path.join("does_not_exist");
    let mut session =
        McpTestSession::with_envs(&[("TOPOS_MCP_FILE_ROOT", project_path.as_os_str())]);

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_evaluate_project",
            "arguments": {
                "path": missing_dir.to_string_lossy(),
                "no_composable": true
            },
            "_meta": meta_2026_with_tasks()
        }
    }));

    let resp = session.recv();
    let res = &resp["result"];
    assert_eq!(
        res["resultType"], "complete",
        "fast-path validation error should return complete immediately: {resp:#?}"
    );
    assert!(
        res["structuredContent"]["error"].as_str().is_some(),
        "expected error message in fast-path response: {res:#?}"
    );
}

/// Clients on `2026-07-28` that do NOT advertise `io.modelcontextprotocol/tasks`
/// run heavy tools inline and receive `resultType: "complete"` directly.
#[test]
fn sep2663_legacy_or_non_task_client_runs_inline_to_completion() {
    let (_project, project_path) = make_temp_rust_project();
    let mut session =
        McpTestSession::with_envs(&[("TOPOS_MCP_FILE_ROOT", project_path.as_os_str())]);

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_evaluate_project",
            "arguments": {
                "path": project_path.to_string_lossy(),
                "no_composable": true
            },
            "_meta": meta_2026_no_tasks()
        }
    }));

    let resp = session.recv();
    let res = &resp["result"];
    assert_eq!(
        res["resultType"], "complete",
        "non-task client must receive complete result directly: {resp:#?}"
    );
    assert_eq!(
        res["structuredContent"]["file_count"], 1,
        "unexpected inline response: {res:#?}"
    );
}

#[cfg(unix)]
fn install_fake_gitnexus_script(bin_dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let script_path = bin_dir.join("gitnexus");
    let script = "#!/bin/sh\necho $$ > \"$PID_FILE\"\ntrap 'exit 130' TERM INT\nsleep 60\n";
    std::fs::write(&script_path, script).expect("write fake gitnexus script");
    let mut perms = std::fs::metadata(&script_path)
        .expect("stat fake gitnexus script")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).expect("chmod +x fake gitnexus script");
}

#[cfg(unix)]
fn spawn_fake_gitnexus_session(
    project_path: &Path,
) -> (tempfile::TempDir, PathBuf, McpTestSession) {
    let fake_bin = tempfile::tempdir().expect("create fake bin dir");
    install_fake_gitnexus_script(fake_bin.path());
    let pid_file = fake_bin.path().join("gitnexus.pid");

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![fake_bin.path().to_path_buf()];
    path_entries.extend(std::env::split_paths(&inherited_path));
    let combined_path = std::env::join_paths(path_entries).expect("join PATH");

    let session = McpTestSession::with_envs(&[
        ("PATH", combined_path.as_os_str()),
        ("PID_FILE", pid_file.as_os_str()),
        ("TOPOS_MCP_FILE_ROOT", project_path.as_os_str()),
    ]);
    (fake_bin, pid_file, session)
}

#[cfg(unix)]
fn read_positive_pid(pid_file: &Path) -> Option<i32> {
    std::fs::read_to_string(pid_file)
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|&pid| pid > 0)
}

#[cfg(unix)]
fn wait_for_recorded_pid(pid_file: &Path, timeout: Duration) -> i32 {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(pid) = read_positive_pid(pid_file) {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for fake gitnexus to record PID at {}",
        pid_file.display()
    );
}

#[cfg(unix)]
fn wait_for_pid_death(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        // SAFETY: `kill(pid, 0)` sends no signal; it only checks whether `pid` is alive.
        if unsafe { libc::kill(pid, 0) != 0 } {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe { libc::kill(pid, 0) != 0 }
}

/// Canceling an in-flight `topos_generate_depgraph` task via `tasks/cancel`
/// transitions the task to `"cancelled"` and kills the `gitnexus` subprocess group.
#[cfg(unix)]
#[test]
fn sep2663_task_cancel_kills_gitnexus_subprocess() {
    let (_project, project_path) = make_temp_rust_project();
    let (_fake_bin, pid_file, mut session) = spawn_fake_gitnexus_session(&project_path);

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_generate_depgraph",
            "arguments": {
                "directory": project_path.to_string_lossy(),
                "force": true
            },
            "_meta": meta_2026_with_tasks()
        }
    }));

    let create_resp = session.recv();
    let create_res = &create_resp["result"];
    assert_eq!(
        create_res["resultType"], "task",
        "expected task response for topos_generate_depgraph: {create_resp:#?}"
    );
    let task_id = create_res["taskId"]
        .as_str()
        .expect("taskId string")
        .to_string();

    let pid = wait_for_recorded_pid(&pid_file, Duration::from_secs(5));

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tasks/cancel",
        "params": {
            "taskId": task_id,
            "_meta": meta_2026_with_tasks()
        }
    }));
    let cancel_resp = session.recv();
    assert!(
        cancel_resp.get("error").is_none(),
        "tasks/cancel returned an error: {cancel_resp:#?}"
    );

    poll_until_status(&mut session, &task_id, "cancelled", Duration::from_secs(5));

    assert!(
        wait_for_pid_death(pid, Duration::from_secs(1)),
        "fake gitnexus subprocess (pid {pid}) remained alive after tasks/cancel"
    );
}

/// Closing stdio (`EOF`) while `topos_generate_depgraph` has a `gitnexus` child
/// running triggers server shutdown and kills the child process group before
/// `topos-mcp` exits.
#[cfg(unix)]
#[test]
fn stdio_eof_kills_in_flight_gitnexus_subprocess() {
    let (_project, project_path) = make_temp_rust_project();
    let (_fake_bin, pid_file, mut session) = spawn_fake_gitnexus_session(&project_path);

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_generate_depgraph",
            "arguments": {
                "directory": project_path.to_string_lossy(),
                "force": true
            },
            "_meta": meta_2026_no_tasks()
        }
    }));

    let pid = wait_for_recorded_pid(&pid_file, Duration::from_secs(5));

    // Trigger stdio EOF by dropping the child's stdin pipe.
    drop(session.stdin.take());

    let _status = session.wait();
    assert!(
        wait_for_pid_death(pid, Duration::from_secs(1)),
        "fake gitnexus subprocess (pid {pid}) survived topos-mcp stdio EOF"
    );
}

/// A git repository with two commits, so `HEAD~1...HEAD` has a file to score.
fn make_git_project_with_history() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp project dir");
    let root = dir.path().canonicalize().expect("canonicalize temp dir");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&root)
            .stdout(Stdio::null())
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("a.py"), "def a(x):\n    return x\n").expect("write a.py");
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    std::fs::write(root.join("a.py"), "def a(x):\n    return x + 1\n").expect("write a.py");
    git(&["add", "."]);
    git(&["commit", "-qm", "change"]);
    (dir, root)
}

fn pr_recap_call(root: &Path, meta: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "topos_pr_recap",
            "arguments": {
                "directory": root.to_string_lossy(),
                "base": "HEAD~1",
                "head": "HEAD",
                "no_coupling": true
            },
            "_meta": meta
        }
    })
}

fn assert_names_the_work(message: &str) {
    assert!(
        message.starts_with("Operation timed out after 0s while "),
        "{message}"
    );
    assert!(
        message.contains("`max_files`") && message.contains("`paths`"),
        "{message}"
    );
}

/// A recap that outruns its deadline, run inline, says what it was doing.
#[test]
fn a_pr_recap_timeout_names_the_work_when_run_inline() {
    let (_project, root) = make_git_project_with_history();
    let mut session = McpTestSession::with_envs(&[
        ("TOPOS_MCP_FILE_ROOT", root.as_os_str()),
        ("TOPOS_PR_RECAP_TIMEOUT", OsStr::new("0.001")),
    ]);
    session.send(&pr_recap_call(&root, meta_2026_no_tasks()));
    let resp = session.recv();
    let message = resp["error"]["message"]
        .as_str()
        .expect("an error response");
    assert_names_the_work(message);
}

/// The same, run as a task: the failed task carries the same message.
#[test]
fn a_pr_recap_timeout_names_the_work_when_run_as_a_task() {
    let (_project, root) = make_git_project_with_history();
    let mut session = McpTestSession::with_envs(&[
        ("TOPOS_MCP_FILE_ROOT", root.as_os_str()),
        ("TOPOS_PR_RECAP_TIMEOUT", OsStr::new("0.001")),
    ]);
    session.send(&pr_recap_call(&root, meta_2026_with_tasks()));
    let resp = session.recv();
    let task_id = resp["result"]["taskId"]
        .as_str()
        .expect("a task")
        .to_string();
    let failed = poll_until_status(&mut session, &task_id, "failed", Duration::from_secs(10));
    let message = failed["error"]["message"].as_str().expect("an error");
    assert_names_the_work(message);
}
