//! `topos_compare_files` parses each file in the language implied by its suffix.
//!
//! The tool used to hardcode Python, so a Rust pair failed to parse while
//! `topos compare` on the same files succeeded.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "topos-mcp-compare-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".git")).expect("scratch dir");
        Self { dir }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn exchange(cwd: &Path, frames: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
        .current_dir(cwd)
        .env_remove("TOPOS_MCP_FILE_ROOT")
        .env("TOPOS_NO_UPDATE_NOTICES", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn topos-mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    for frame in frames {
        writeln!(stdin, "{frame}").expect("write frame");
    }
    drop(stdin);

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
        .find(|response| response["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {responses:#?}"))
        .get("result")
        .unwrap_or_else(|| panic!("response {id} carried no result: {responses:#?}"))
}

#[test]
fn compare_files_parses_rust_over_stdio() {
    let scratch = Scratch::new();
    std::fs::write(scratch.dir.join("a.rs"), "fn alpha() -> i32 { 1 }\n").unwrap();
    std::fs::write(scratch.dir.join("b.rs"), "fn beta() -> i32 { 2 }\n").unwrap();

    let responses = exchange(
        &scratch.dir,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"compare-test","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"topos_compare_files","arguments":{"source":"a.rs","target":"b.rs"}}}"#,
        ],
    );

    let call = result_for_id(&responses, 2);
    assert_eq!(call["isError"], false, "{call:#?}");
    let structured = &call["structuredContent"];
    assert_eq!(structured["source_valid"], true, "{structured:#?}");
    assert_eq!(structured["target_valid"], true, "{structured:#?}");
    assert!(structured["error"].is_null(), "{structured:#?}");
    assert!(
        structured["similarity"].as_f64().unwrap_or(0.0) > 0.0,
        "{structured:#?}"
    );
}
