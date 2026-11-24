//! `topos evaluate` through the real binary.

use std::fs;
use std::process::{Command, Stdio};

#[test]
fn evaluate_accepts_a_file_name_with_multi_byte_characters() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("src/ünîcödé");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("fîlé.ts"), "export const a = 1;\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(["evaluate", "src/ünîcödé/fîlé.ts", "--json"])
        .current_dir(root.path())
        .env("PATH", "/usr/bin:/bin")
        .env("TOPOS_CACHE_INDEX_DIR", root.path().join(".cache-index"))
        .env_remove("TOPOS_MCP_FILE_ROOT")
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["results"][0]["language"], "typescript", "{report}");
}
