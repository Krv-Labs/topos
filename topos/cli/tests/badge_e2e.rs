use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn badge(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_topos"))
        .arg("badge")
        .args(args)
        .current_dir(cwd)
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
