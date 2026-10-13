//! Stdio lifecycle regression tests — both MCP eras Topos claims.
//!
//! The rmcp 3 upgrade (#264) got `server/discover` for free from the SDK's
//! default `ServerHandler` impls, which is exactly why it needs a test: nothing
//! in the Topos source mentions discovery, so a future handler change could
//! silently drop the stateless path with no compile error. These pipe real
//! JSON-RPC frames into the built binary and assert on what comes back.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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
    fn new() -> Self {
        Self::with_envs::<&str, &str>(&[])
    }

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
            if !line.trim().is_empty() {
                return serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("invalid JSON-RPC line ({e}): {line}"));
            }
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

/// Pipe newline-delimited frames into a fresh `topos-mcp`, close stdin, and
/// parse the responses it wrote before shutting down.
fn exchange(frames: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn topos-mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    for frame in frames {
        writeln!(stdin, "{frame}").expect("write frame");
    }
    drop(stdin); // EOF — the server shuts the stdio transport down.

    let out = child.wait_with_output().expect("wait for topos-mcp");
    String::from_utf8(out.stdout)
        .expect("utf-8 stdout")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response line"))
        .collect()
}

fn result_for_id(responses: &[serde_json::Value], id: u64) -> &serde_json::Value {
    responses
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {responses:#?}"))
        .get("result")
        .unwrap_or_else(|| panic!("response {id} carried no result: {responses:#?}"))
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
fn make_temp_rust_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp project dir");
    std::fs::create_dir(dir.path().join(".git")).expect("create .git marker");
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .expect("write lib.rs");
    dir
}

/// Legacy initialize era — what every host in the wild speaks today.
#[test]
fn initialize_lifecycle_lists_and_calls_tools() {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"lifecycle-test","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"topos_get_doc","arguments":{"topic":"agent-contract"}}}"#,
    ]);

    let init = result_for_id(&responses, 1);
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "topos_mcp");

    let names = tool_names(result_for_id(&responses, 2));
    assert!(
        names.iter().any(|n| n == "topos_get_doc"),
        "topos_get_doc missing from {names:?}"
    );

    let call = result_for_id(&responses, 3);
    assert_eq!(call["isError"], false);
    assert!(
        call["content"][0]["text"]
            .as_str()
            .expect("text content")
            .contains("Topos Agent Contract"),
        "unexpected doc body: {call:#?}"
    );
}

/// Stateless 2026-07-28 era — `server/discover` as the very first frame, then
/// tool traffic with no `initialize` and no session id anywhere.
#[test]
fn discover_lifecycle_needs_no_initialize() {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        // Under 2026-07-28 every request is self-describing: `_meta` carries
        // the protocol version AND client capabilities, or the server rejects
        // it with -32602. There is no handshake to hoist them out of.
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"topos_get_doc","arguments":{"topic":"agent-contract"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
    ]);

    let discover = result_for_id(&responses, 1);
    assert_eq!(discover["resultType"], "complete");
    // The pinned `SUPPORTED_PROTOCOL_VERSIONS`, reaching the wire intact. This
    // list also bounds `initialize` negotiation, so dropping a revision here
    // stops Topos speaking it — see `initialize_negotiates_every_supported_revision`.
    // Asserted exactly (not by presence) so that an rmcp upgrade which adds a
    // revision fails here and forces a deliberate choice.
    assert_eq!(
        discover["supportedVersions"],
        serde_json::json!([
            "2024-11-05",
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
            "2026-07-28"
        ])
    );
    assert_eq!(
        discover["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "topos_mcp"
    );
    for capability in ["tools", "resources", "prompts"] {
        assert!(
            discover["capabilities"].get(capability).is_some(),
            "{capability} capability missing from discover: {discover:#?}"
        );
    }
    assert!(
        discover["capabilities"]["extensions"]
            .get("io.modelcontextprotocol/tasks")
            .is_some(),
        "io.modelcontextprotocol/tasks extension capability missing from discover: {discover:#?}"
    );

    // Same tool surface as the initialize path: discovery is a lifecycle
    // change, not a different server.
    assert_eq!(
        tool_names(result_for_id(&responses, 2)),
        tool_names(&initialize_era_tools_list())
    );

    let call = result_for_id(&responses, 3);
    assert_eq!(call["isError"], false);
    assert!(call["content"][0]["text"]
        .as_str()
        .expect("text content")
        .contains("Topos Agent Contract"));
}

/// Every handshake revision in `SUPPORTED_PROTOCOL_VERSIONS` negotiates as
/// itself over `initialize`, while `2026-07-28` (`ProtocolVersion::NO_INITIALIZE`)
/// replaced `initialize` with `server/discover` + per-request `_meta` and
/// therefore answers a legacy `initialize` call with the newest handshake
/// revision (`2025-11-25`).
#[test]
fn initialize_negotiates_every_supported_revision() {
    // Hoisted: the reference surface is the same for every revision, so spawning
    // it once per iteration would double the child processes for nothing.
    let reference_tools = tool_names(&initialize_era_tools_list());
    for (requested, expected_negotiated) in [
        ("2024-11-05", "2024-11-05"),
        ("2025-03-26", "2025-03-26"),
        ("2025-06-18", "2025-06-18"),
        ("2025-11-25", "2025-11-25"),
        ("2026-07-28", "2025-11-25"),
    ] {
        let responses = exchange(&[
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{requested}","capabilities":{{}},"clientInfo":{{"name":"lifecycle-test","version":"0.0.1"}}}}}}"#
            ),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ]);

        let init = result_for_id(&responses, 1);
        assert_eq!(
            init["protocolVersion"], expected_negotiated,
            "unexpected negotiated version for requested {requested}: {init:#?}"
        );
        // A revision is only genuinely supported if the session then works.
        assert_eq!(
            tool_names(result_for_id(&responses, 2)),
            reference_tools,
            "tool surface differs under requested {requested}"
        );
    }
}

/// SEP-2549 explicit caching hints: static discovery/doc surfaces advertise a
/// 24h public TTL, while runtime-specific `topos://build` advertises `ttlMs: 0`
/// and `cacheScope: "private"`.
#[test]
fn sep2549_caching_hints_on_static_and_dynamic_surfaces() {
    let mut session = McpTestSession::new();
    let meta = meta_2026_no_tasks();

    for (id, method, extra) in [
        (1u64, "tools/list", serde_json::json!({})),
        (2u64, "prompts/list", serde_json::json!({})),
        (3u64, "resources/list", serde_json::json!({})),
        (4u64, "resources/templates/list", serde_json::json!({})),
        (
            5u64,
            "resources/read",
            serde_json::json!({ "uri": "topos://docs/lattice" }),
        ),
    ] {
        let mut params = extra.as_object().cloned().unwrap_or_default();
        params.insert("_meta".to_string(), meta.clone());
        session.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let resp = session.recv();
        let res = &resp["result"];
        assert_eq!(
            res["resultType"], "complete",
            "{method} missing resultType=complete: {resp:#?}"
        );
        assert_eq!(
            res["ttlMs"], 86_400_000,
            "{method} expected 24h ttlMs: {resp:#?}"
        );
        assert_eq!(
            res["cacheScope"], "public",
            "{method} expected public cacheScope: {resp:#?}"
        );
    }

    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "resources/read",
        "params": {
            "uri": "topos://build",
            "_meta": meta,
        }
    }));
    let build_resp = session.recv();
    let build_res = &build_resp["result"];
    assert_eq!(build_res["resultType"], "complete");
    assert_eq!(build_res["ttlMs"], 0);
    assert_eq!(build_res["cacheScope"], "private");
}

/// SEP-2663 task creation, polling (`tasks/get`), and completion output on
/// `2026-07-28` when the client declares `io.modelcontextprotocol/tasks`.
#[test]
fn sep2663_task_lifecycle_create_poll_complete() {
    let project = make_temp_rust_project();
    let project_path = project
        .path()
        .canonicalize()
        .expect("canonicalize temp project");
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

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut req_id = 2u64;
    let completed = loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for task {task_id} to complete"
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
        match poll_res["status"].as_str() {
            Some("completed") => break poll_res.clone(),
            Some("working") => std::thread::sleep(Duration::from_millis(20)),
            other => panic!("unexpected task status {other:?}: {poll_resp:#?}"),
        }
    };

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
    let project = make_temp_rust_project();
    let project_path = project
        .path()
        .canonicalize()
        .expect("canonicalize temp project");
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
        "fast-path validation error should return complete immediately, not spawn a task: {resp:#?}"
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
    let project = make_temp_rust_project();
    let project_path = project
        .path()
        .canonicalize()
        .expect("canonicalize temp project");
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
fn wait_for_recorded_pid(pid_file: &Path, timeout: Duration) -> i32 {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(raw) = std::fs::read_to_string(pid_file) {
            if let Ok(pid) = raw.trim().parse::<i32>() {
                if pid > 0 {
                    return pid;
                }
            }
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
    let project = make_temp_rust_project();
    let project_path = project
        .path()
        .canonicalize()
        .expect("canonicalize temp project");
    let fake_bin = tempfile::tempdir().expect("create fake bin dir");
    install_fake_gitnexus_script(fake_bin.path());
    let pid_file = fake_bin.path().join("gitnexus.pid");

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![fake_bin.path().to_path_buf()];
    path_entries.extend(std::env::split_paths(&inherited_path));
    let combined_path = std::env::join_paths(path_entries).expect("join PATH");

    let mut session = McpTestSession::with_envs(&[
        ("PATH", combined_path.as_os_str()),
        ("PID_FILE", pid_file.as_os_str()),
        ("TOPOS_MCP_FILE_ROOT", project_path.as_os_str()),
    ]);

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

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut req_id = 3u64;
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for cancelled task status"
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
        match poll_res["status"].as_str() {
            Some("cancelled") => break,
            Some("working") => std::thread::sleep(Duration::from_millis(20)),
            other => panic!("unexpected status after cancel: {other:?} ({poll_resp:#?})"),
        }
    }

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
    let project = make_temp_rust_project();
    let project_path = project
        .path()
        .canonicalize()
        .expect("canonicalize temp project");
    let fake_bin = tempfile::tempdir().expect("create fake bin dir");
    install_fake_gitnexus_script(fake_bin.path());
    let pid_file = fake_bin.path().join("gitnexus.pid");

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![fake_bin.path().to_path_buf()];
    path_entries.extend(std::env::split_paths(&inherited_path));
    let combined_path = std::env::join_paths(path_entries).expect("join PATH");

    let mut session = McpTestSession::with_envs(&[
        ("PATH", combined_path.as_os_str()),
        ("PID_FILE", pid_file.as_os_str()),
        ("TOPOS_MCP_FILE_ROOT", project_path.as_os_str()),
    ]);

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

fn tool_names(list_result: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = list_result["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list carried no array: {list_result:#?}"))
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_string())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "tools/list returned nothing");
    names
}

fn initialize_era_tools_list() -> serde_json::Value {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"lifecycle-test","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    ]);
    result_for_id(&responses, 2).clone()
}
