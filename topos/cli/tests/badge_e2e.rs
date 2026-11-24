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

fn write_tangle(root: &Path) {
    let mut source = String::from(
        "def tangle(a, b, c, d):\n    \"\"\"Branch on everything.\"\"\"\n    total = 0\n",
    );
    for i in 0..12 {
        source.push_str(&format!(
            "    if a > {i}:\n        if b > {i}:\n            if c > {i}:\n                \
             if d > {i}:\n                    total += {i}\n                else:\n                    \
             total -= {i}\n"
        ));
    }
    source.push_str("    return total\n");
    fs::write(root.join("src/tangle.py"), source).unwrap();
}

fn one_failing_of_four() -> tempfile::TempDir {
    let root = fixture();
    write_module(root.path(), "sub.py");
    write_module(root.path(), "mul.py");
    write_tangle(root.path());
    root
}

fn json_of(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn writes_an_svg_and_a_shields_endpoint_for_a_directory() {
    let root = fixture();
    let result = json_of(&badge(
        root.path(),
        &[
            "--no-composable",
            "--out",
            "badges/topos.svg",
            "--endpoint",
            "badges/topos.json",
            "--json",
        ],
    ));
    assert_eq!(result["files"], 1);
    assert_eq!(result["unparsed"], 0);
    assert_eq!(
        result["pillars"]["composable"],
        json!({"passed": 0, "measured": 0, "unmeasured": 1, "rate": null, "state": "not_measured"})
    );
    for pillar in ["simple", "secure", "navigable"] {
        assert_eq!(
            result["pillars"][pillar],
            json!({"passed": 1, "measured": 1, "unmeasured": 0, "rate": 100, "state": "measured"}),
            "{pillar}"
        );
    }
    assert_eq!(result["strict"]["medal"], "GOLD", "{result:#}");

    let endpoint: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join("badges/topos.json")).unwrap())
            .unwrap();
    assert_eq!(
        endpoint,
        json!({
            "schemaVersion": 1,
            "label": "topos",
            "message": "SIMPLE 100% · COMPOSABLE n/a · SECURE 100% · NAVIGABLE 100%",
            "color": "44cc11",
        })
    );

    let svg = fs::read_to_string(root.path().join("badges/topos.svg")).unwrap();
    assert_well_formed(&svg);
    assert!(
        svg.contains(
            "<title>topos: SIMPLE 100%, COMPOSABLE n/a, SECURE 100%, NAVIGABLE 100%</title>"
        ),
        "{svg}"
    );
    for code in [">Si<", ">Co<", ">Se<", ">Na<"] {
        assert!(svg.contains(code), "missing {code:?}");
    }
    assert!(
        !svg.contains("GOLD"),
        "the medal belongs to `topos evaluate`, not the badge"
    );
}

#[test]
fn one_failing_file_of_four_shows_as_a_pass_rate_everywhere() {
    let root = one_failing_of_four();
    let output = badge(
        root.path(),
        &[
            "--no-composable",
            "--out",
            "topos.svg",
            "--endpoint",
            "topos.json",
            "--json",
        ],
    );
    let result = json_of(&output);
    assert_eq!(result["files"], 4);
    assert_eq!(
        result["pillars"]["simple"],
        json!({"passed": 3, "measured": 4, "unmeasured": 0, "rate": 75, "state": "measured"}),
        "{result:#}"
    );
    assert_eq!(result["pillars"]["secure"]["rate"], 100, "{result:#}");
    let navigable = result["pillars"]["navigable"]["rate"].as_u64().unwrap();
    let navigable = format!("NAVIGABLE {navigable}%");

    let svg = fs::read_to_string(root.path().join("topos.svg")).unwrap();
    assert_well_formed(&svg);
    assert!(svg.contains("topos: SIMPLE 75%, "), "{svg}");
    assert!(svg.contains(&format!(", {navigable}</title>")), "{svg}");

    let endpoint: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join("topos.json")).unwrap()).unwrap();
    let message = endpoint["message"].as_str().unwrap();
    assert!(
        message.starts_with("SIMPLE 75% · COMPOSABLE n/a · SECURE 100% · "),
        "{message}"
    );
    assert!(message.ends_with(&navigable), "{message}");
    assert_eq!(endpoint["color"], "dfb317", "75% is the lowest rate here");

    let status = stderr(&output);
    assert!(
        status.contains(&format!(
            "SIMPLE 75% · COMPOSABLE n/a · SECURE 100% · {navigable} (4 files)"
        )),
        "{status}"
    );
}

#[test]
fn prints_the_svg_when_no_output_is_named() {
    let root = fixture();
    let output = badge(root.path(), &["--no-composable"]);
    assert!(output.status.success(), "{output:?}");
    let svg = String::from_utf8(output.stdout).unwrap();
    assert_well_formed(&svg);
    assert!(svg.contains(
        "aria-label=\"topos: SIMPLE 100%, COMPOSABLE n/a, SECURE 100%, NAVIGABLE 100%\""
    ));
}

#[test]
fn an_unparsed_file_is_counted_apart_and_left_out_of_every_rate() {
    let root = fixture();
    fs::write(root.path().join("src/broken.py"), "def broken(:\n").unwrap();
    let output = badge(root.path(), &["--no-composable", "--json"]);
    let result = json_of(&output);
    assert_eq!(result["files"], 2);
    assert_eq!(result["unparsed"], 1);
    assert_eq!(result["pillars"]["simple"]["measured"], 1, "{result:#}");
    assert_eq!(result["pillars"]["simple"]["rate"], 100, "{result:#}");
    assert_eq!(result["strict"]["medal"], "NO MEDAL", "{result:#}");
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
fn a_graph_that_covers_every_file_measures_composable() {
    let root = graph_fixture(&["add.py", "sub.py"]);
    let result = json_of(&badge(root.path(), &["--json"]));
    assert_eq!(
        result["pillars"]["composable"],
        json!({"passed": 2, "measured": 2, "unmeasured": 0, "rate": 100, "state": "measured"}),
        "{result:#}"
    );
    assert_eq!(result["strict"]["medal"], "PLATINUM", "{result:#}");
    assert_eq!(result["files"], 2);
}

#[test]
fn a_graph_that_misses_a_file_rates_composable_over_the_covered_files() {
    let root = graph_fixture(&["add.py"]);
    let output = badge(root.path(), &["--json"]);
    let result = json_of(&output);
    assert_eq!(
        result["pillars"]["composable"],
        json!({"passed": 1, "measured": 1, "unmeasured": 1, "rate": 100, "state": "partial"}),
        "{result:#}"
    );
    assert_eq!(result["strict"]["medal"], "GOLD", "{result:#}");
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
    assert_eq!(json_of(&output)["files"], 1);
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
    assert!(
        status.contains("SIMPLE 100% · COMPOSABLE n/a · SECURE 100% · NAVIGABLE 100% (1 file)"),
        "{status}"
    );

    let output = badge(root.path(), &["--no-composable", "--out", "-", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("--out - cannot be combined with --json"));
    let output = badge(root.path(), &["--no-composable", "--endpoint", "-"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("--endpoint needs a file path"));
}
