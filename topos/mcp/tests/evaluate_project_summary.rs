//! `topos_evaluate_project` over a real stdio session: the summary default,
//! `detail: "full"`, and the warning for a very large tree.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct McpSession {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn start(file_root: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
            .env("TOPOS_MCP_FILE_ROOT", file_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn topos-mcp");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    /// Calls `topos_evaluate_project` and returns the JSON-RPC `result`.
    fn evaluate_project(&mut self, arguments: serde_json::Value) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "topos_evaluate_project",
                "arguments": arguments,
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        let stdin = self.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{request}").expect("write request");
        stdin.flush().expect("flush request");
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.stdout.read_line(&mut line).expect("read response");
            assert!(read > 0, "unexpected EOF from topos-mcp");
            if line.trim().is_empty() {
                continue;
            }
            let response: serde_json::Value = serde_json::from_str(&line).expect("JSON-RPC line");
            return response["result"].clone();
        }
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A project with `files` one-line Python files spread over seven
/// top-level directories.
fn large_project(files: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::create_dir(dir.path().join(".git")).expect(".git marker");
    for i in 0..files {
        let sub = dir.path().join(format!("d{}", i % 7));
        std::fs::create_dir_all(&sub).expect("subdirectory");
        std::fs::write(sub.join(format!("f{i}.py")), "def f():\n    return 1\n").expect("write");
    }
    let root = dir.path().canonicalize().expect("canonicalize");
    (dir, root)
}

#[test]
fn a_tree_past_a_thousand_files_warns_with_the_top_level_directories() {
    let (_project, root) = large_project(1_001);
    let mut session = McpSession::start(&root);
    let result = session.evaluate_project(serde_json::json!({
        "path": root.to_string_lossy(),
        "no_composable": true
    }));

    let structured = &result["structuredContent"];
    assert_eq!(
        structured["schema"], "topos.evaluate_project.summary.v1",
        "{result:#?}"
    );
    assert_eq!(structured["file_count"], 1_001);
    let warning = structured["warnings"][0].as_str().expect("first warning");
    assert!(
        warning.starts_with(
            "1001 files found; top 5 of 7 top-level directories by scored-file count: d0 143"
        ),
        "{warning}"
    );
    let text = result["content"][0]["text"].as_str().expect("text block");
    assert!(
        text.contains(warning),
        "text-only clients get the warning:\n{text}"
    );
    assert!(text.len() < 2_500, "{} chars", text.len());
}

#[test]
fn full_detail_over_stdio_returns_absolute_rows_for_the_same_tree() {
    let (_project, root) = large_project(8);
    let mut session = McpSession::start(&root);
    let result = session.evaluate_project(serde_json::json!({
        "path": root.to_string_lossy(),
        "no_composable": true,
        "detail": "full",
        "limit": 3
    }));
    let structured = &result["structuredContent"];
    assert_eq!(structured["files"].as_array().map(Vec::len), Some(3));
    for file in structured["files"].as_array().unwrap() {
        let path = file["filepath"].as_str().unwrap();
        assert!(
            std::path::Path::new(path).is_absolute() && path.starts_with(root.to_str().unwrap()),
            "{path}"
        );
    }
    assert!(structured["scan_ms"].is_u64());
}

#[test]
fn an_error_root_resolves_against_the_file_root_and_never_names_a_file() {
    let (_project, root) = large_project(1);
    let mut session = McpSession::start(&root);

    // The server's cwd is not the file root: `nope` still resolves under it.
    let missing = session.evaluate_project(serde_json::json!({"path": "nope"}));
    let structured = &missing["structuredContent"];
    assert_eq!(
        structured["root"],
        root.join("nope").to_string_lossy().as_ref()
    );
    assert!(structured["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(structured.get("aggregate_floor_verdict").is_none());

    // A file is not a root.
    let file = root.join("d0").join("f0.py");
    let not_dir = session.evaluate_project(serde_json::json!({"path": file.to_string_lossy()}));
    let structured = &not_dir["structuredContent"];
    assert!(structured["error"]
        .as_str()
        .unwrap()
        .contains("not a directory"));
    assert!(structured.get("root").is_none(), "{structured}");
}
