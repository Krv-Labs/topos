//! File spellings must not change COMPOSABLE measurements.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn run(cwd: &Path, args: &[&str]) -> Value {
    let cache_index = cwd.join(".topos-test-cache-index");
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        // Keep an installed GitNexus from regenerating the fixture store.
        .env("PATH", "/usr/bin:/bin")
        .env("TOPOS_CACHE_INDEX_DIR", cache_index)
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

fn file_node(path: &str, content: &str) -> Value {
    json!({
        "id": format!("File:{path}"),
        "label": "File",
        "properties": {"filePath": path, "content": content}
    })
}

fn function_node(path: &str, name: &str, exported: bool) -> Value {
    json!({
        "id": format!("Function:{path}:{name}"),
        "label": "Function",
        "properties": {"name": name, "filePath": path, "isExported": exported}
    })
}

fn edge(rel_type: &str, source: &str, target: &str) -> Value {
    json!({"type": rel_type, "sourceId": source, "targetId": target})
}

#[test]
fn composable_fixture_keeps_its_real_call_and_drops_a_false_cross_crate_one() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/composable_mdg");
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tools/src")).unwrap();
    let lib = fs::read_to_string(fixture.join("src/lib.rs")).unwrap();
    let helper = fs::read_to_string(fixture.join("src/helper.rs")).unwrap();
    let tools = "fn write() {}\n";
    let manifest = "[package]\nname = \"fixture\"\n";
    fs::write(root.path().join("src/lib.rs"), &lib).unwrap();
    fs::write(root.path().join("src/helper.rs"), &helper).unwrap();
    fs::write(root.path().join("tools/src/io.rs"), tools).unwrap();
    fs::write(root.path().join("Cargo.toml"), manifest).unwrap();
    fs::write(root.path().join("tools/Cargo.toml"), manifest).unwrap();

    let store = root.path().join(".gitnexus/lbug");
    fs::create_dir_all(&store).unwrap();
    fs::write(
        store.join("nodes.json"),
        json!([
            file_node("Cargo.toml", manifest),
            file_node("tools/Cargo.toml", manifest),
            file_node("src/lib.rs", &lib),
            file_node("src/helper.rs", &helper),
            file_node("tools/src/io.rs", tools),
            function_node("src/lib.rs", "entry", true),
            function_node("src/helper.rs", "assist", true),
            function_node("tools/src/io.rs", "write", false),
            edge("DEFINES", "File:src/lib.rs", "Function:src/lib.rs:entry"),
            edge(
                "DEFINES",
                "File:src/helper.rs",
                "Function:src/helper.rs:assist"
            ),
            edge(
                "DEFINES",
                "File:tools/src/io.rs",
                "Function:tools/src/io.rs:write"
            ),
            edge(
                "CALLS",
                "Function:src/lib.rs:entry",
                "Function:src/helper.rs:assist"
            ),
            edge(
                "CALLS",
                "Function:src/lib.rs:entry",
                "Function:tools/src/io.rs:write"
            ),
        ])
        .to_string(),
    )
    .unwrap();

    let inspected = run(root.path(), &["inspect", "src/lib.rs"]);
    assert_eq!(inspected["dimensions"]["composable"], "COMPOSABLE");
    assert_eq!(inspected["raw_metrics"]["mdg.fan_out"], 1.0);

    let status = run(root.path(), &["depgraph", "status"]);
    assert_eq!(
        status["dropped_edge_counts"]["rust_private_cross_crate"], 1,
        "{status}"
    );
    assert_eq!(status["dropped_edges"][0]["source_file"], "src/lib.rs");
    assert_eq!(status["dropped_edges"][0]["target_file"], "tools/src/io.rs");
}
