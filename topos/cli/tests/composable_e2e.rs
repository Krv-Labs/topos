//! File spellings must not change COMPOSABLE measurements.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn run(cwd: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        // Keep an installed GitNexus from regenerating the fixture store.
        .env("PATH", "/usr/bin:/bin")
        .env_remove("TOPOS_MCP_FILE_ROOT")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn indexed_files_keep_composable_with_relative_paths_from_a_subdirectory() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    let store = root.path().join(".gitnexus/lbug");
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&store).unwrap();
    fs::write(src.join("f.py"), "def f():\n    return 1\n").unwrap();
    fs::write(
        store.join("nodes.json"),
        json!([
            {
                "id": "File:src/f.py",
                "label": "File",
                "properties": {"filePath": "src/f.py"}
            },
            {
                "id": "File:src/dependency.py",
                "label": "File",
                "properties": {"filePath": "src/dependency.py"}
            },
            {
                "type": "IMPORTS",
                "sourceId": "File:src/f.py",
                "targetId": "File:src/dependency.py"
            }
        ])
        .to_string(),
    )
    .unwrap();

    let absolute = src.join("f.py");
    let expected = run(&src, &["inspect", absolute.to_str().unwrap()]);
    assert_eq!(expected["dimensions"]["composable"], "COMPOSABLE");
    assert_eq!(expected["raw_metrics"]["mdg.coupling"], 1.0);
    for path in ["f.py", "./f.py", "../src/f.py"] {
        let actual = run(&src, &["inspect", path]);
        assert_eq!(actual["dimensions"], expected["dimensions"], "{path}");
        assert_eq!(actual["raw_metrics"], expected["raw_metrics"], "{path}");
    }

    let expected = run(&src, &["evaluate", absolute.to_str().unwrap()]);
    assert_eq!(
        expected["results"][0]["dimensions"]["composable"],
        "COMPOSABLE"
    );
    for args in [vec!["evaluate", "./f.py"], vec!["evaluate", ".", "-r"]] {
        let actual = run(&src, &args);
        assert_eq!(
            actual["results"][0]["dimensions"],
            expected["results"][0]["dimensions"]
        );
        assert_eq!(
            actual["results"][0]["raw_metrics"],
            expected["results"][0]["raw_metrics"]
        );
    }
}
