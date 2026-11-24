use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn badge(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_topos"))
        .arg("badge")
        .args(args)
        .current_dir(cwd)
        .env("PATH", "/usr/bin:/bin")
        .env("TOPOS_CACHE_INDEX_DIR", cwd.join(".topos-test-cache-index"))
        .env_remove("TOPOS_MCP_FILE_ROOT")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("add.py"),
        "def add(left, right):\n    \"\"\"Add two numbers.\"\"\"\n    return left + right\n",
    )
    .unwrap();
    root
}

fn write_module(root: &Path, name: &str) {
    fs::write(
        root.join("src").join(name),
        "def run(left, right):\n    \"\"\"Combine two numbers.\"\"\"\n    return left + right\n",
    )
    .unwrap();
}

fn graph_fixture(indexed: &[&str]) -> tempfile::TempDir {
    let root = fixture();
    write_module(root.path(), "sub.py");
    let store = root.path().join(".gitnexus/lbug");
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(&store).unwrap();
    let nodes: Vec<Value> = indexed
        .iter()
        .map(|name| {
            let path = format!("src/{name}");
            json!({"id": format!("File:{path}"), "label": "File", "properties": {"filePath": path}})
        })
        .collect();
    fs::write(store.join("nodes.json"), Value::from(nodes).to_string()).unwrap();
    root
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_well_formed(svg: &str) {
    let mut open: Vec<&str> = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        let end = start + rest[start..].find('>').expect("unterminated tag");
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            assert_eq!(open.pop(), Some(name), "mismatched </{name}>");
        } else if !tag.ends_with('/') {
            open.push(tag.split_whitespace().next().expect("empty tag"));
        }
        assert_eq!(tag.matches('"').count() % 2, 0, "unbalanced quotes: {tag}");
    }
    assert!(open.is_empty(), "unclosed tags: {open:?}");
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.ends_with("</svg>\n"));
}

#[test]
fn writes_an_svg_and_a_shields_endpoint_for_a_directory() {
    let root = fixture();
    let output = badge(
        root.path(),
        &[
            "--no-composable",
            "--out",
            "badges/topos.svg",
            "--endpoint",
            "badges/topos.json",
            "--json",
        ],
    );
    assert!(output.status.success(), "{output:?}");

    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["medal"], "GOLD", "{result:#}");
    assert_eq!(result["files"], 1);
    assert_eq!(result["pillars"]["composable"], "not_measured");
    for pillar in ["simple", "secure", "navigable"] {
        assert_eq!(result["pillars"][pillar], "pass", "{pillar}");
    }

    let endpoint: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join("badges/topos.json")).unwrap())
            .unwrap();
    assert_eq!(endpoint["schemaVersion"], 1);
    assert_eq!(endpoint["label"], "topos");
    assert_eq!(endpoint["message"], "GOLD");
    assert_eq!(endpoint["color"], "b45309");

    let svg = fs::read_to_string(root.path().join("badges/topos.svg")).unwrap();
    assert_well_formed(&svg);
    for state in [
        "SIMPLE pass",
        "COMPOSABLE not measured",
        "SECURE pass",
        "NAVIGABLE pass",
    ] {
        assert!(svg.contains(state), "missing {state:?}");
    }
}

#[test]
fn prints_the_svg_when_no_output_is_named() {
    let root = fixture();
    let output = badge(root.path(), &["--no-composable", "--style", "verdict"]);
    assert!(output.status.success(), "{output:?}");
    let svg = String::from_utf8(output.stdout).unwrap();
    assert_well_formed(&svg);
    assert!(svg.contains("aria-label=\"Topos lattice verdict: GOLD\""));
}

#[test]
fn a_failing_project_still_exits_zero() {
    let root = fixture();
    fs::write(root.path().join("src/broken.py"), "def broken(:\n").unwrap();
    let output = badge(root.path(), &["--no-composable", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["medal"], "NO MEDAL", "{result:#}");
}

#[test]
fn a_directory_without_source_files_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let output = badge(root.path(), &["--no-composable"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no supported source files"));
}

#[test]
fn a_graph_that_covers_every_file_can_award_platinum() {
    let root = graph_fixture(&["add.py", "sub.py"]);
    let output = badge(root.path(), &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["pillars"]["composable"], "pass", "{result:#}");
    assert_eq!(result["medal"], "PLATINUM", "{result:#}");
    assert_eq!(result["files"], 2);
}

#[test]
fn a_graph_that_misses_a_file_leaves_composable_unmeasured() {
    let root = graph_fixture(&["add.py"]);
    let output = badge(root.path(), &["--json"]);
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["pillars"]["composable"], "not_measured",
        "{result:#}"
    );
    assert_eq!(result["medal"], "GOLD", "{result:#}");
    let warnings = result["warnings"].to_string();
    assert!(
        warnings.contains("1 of 2 files had no dependency-graph entry"),
        "{warnings}"
    );
    assert!(warnings.contains("topos depgraph generate"), "{warnings}");
    assert!(stderr(&output).contains("1 of 2 files had no dependency-graph entry"));
}

#[test]
fn one_path_for_both_outputs_is_rejected_before_anything_is_written() {
    let root = fixture();
    let output = badge(
        root.path(),
        &[
            "--no-composable",
            "--out",
            "badge.out",
            "--endpoint",
            "./badge.out",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr(&output).contains("--out and --endpoint name the same file"),
        "{output:?}"
    );
    assert!(!root.path().join("badge.out").exists());
}

#[test]
fn a_file_named_twice_is_scored_once() {
    let root = fixture();
    let output = badge(
        root.path(),
        &["src", "./src/add.py", "--no-composable", "--json"],
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["files"], 1, "{result:#}");
}

#[test]
fn written_files_are_named_and_a_dash_means_standard_output() {
    let root = fixture();
    let output = badge(
        root.path(),
        &["--no-composable", "--out", "-", "--endpoint", "topos.json"],
    );
    assert!(output.status.success(), "{output:?}");
    let status = stderr(&output);
    assert_well_formed(&String::from_utf8(output.stdout).unwrap());
    assert!(!root.path().join("-").exists());
    assert!(status.contains("Wrote topos.json"), "{status}");
    assert!(status.contains("(1 file)"), "{status}");

    let output = badge(root.path(), &["--no-composable", "--out", "-", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("--out - cannot be combined with --json"));
    let output = badge(root.path(), &["--no-composable", "--endpoint", "-"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("--endpoint needs a file path"));
}
