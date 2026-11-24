//! `topos pr-recap` on a file whose entropy leaves the SIMPLE band, driven
//! through the real binary against scratch repositories: test and
//! literal-data files warn, ordinary code still blocks, and an emptied
//! file is not a finding at all.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Commit `base`, then `head`, as `path` in a fresh repository, with an
/// optional `.topos.toml`, and return the recap's JSON.
fn recap(path: &str, base: &str, head: &str, config: Option<&str>) -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    let file = repo.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "recap@example.com"]);
    git(&repo, &["config", "user.name", "Recap"]);
    if let Some(config) = config {
        fs::write(repo.join(".topos.toml"), config).unwrap();
    }
    fs::write(&file, base).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    fs::write(&file, head).unwrap();
    git(&repo, &["commit", "-qam", "head"]);
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(["pr-recap", "--base", "HEAD~1", "--head", "HEAD", "--json"])
        .current_dir(&repo)
        .env("NO_COLOR", "1")
        .env("TOPOS_CACHE_INDEX_DIR", dir.path().join("cache-index"))
        .stdin(Stdio::null())
        .output()
        .expect("topos");
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The `pillar_lost` finding on SIMPLE, if any.
fn simple_lost(value: &Value) -> Option<&Value> {
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["gate"] == "pillar_lost" && finding["pillar"].as_str() == Some("simple")
        })
}

const PYTEST_BASE: &str = "from app import add\n\n\ndef test_add_small_numbers():\n    \
    assert add(1, 2) == 3\n\n\ndef test_add_negative_numbers():\n    result = add(-4, -6)\n    \
    assert result == -10, \"negatives should sum\"\n\n\ndef test_add_mixed_signs():\n    \
    values = [(5, -3, 2), (-7, 10, 3)]\n    for left, right, expected in values:\n        \
    assert add(left, right) == expected\n";

/// Sixty near-identical cases appended to [`PYTEST_BASE`].
fn repeated_tests() -> String {
    let cases: String = (1..=60)
        .map(|i| {
            format!(
                "\n\ndef test_add_case_{i}():\n    left, right = {i}, {}\n    \
                 assert add(left, right) == left + right, \"add should sum two integers\"\n",
                i * 2
            )
        })
        .collect();
    format!("{PYTEST_BASE}{cases}")
}

fn dict(entries: usize) -> String {
    let body: String = (1..=entries)
        .map(|i| format!("    \"c{i:03}\": \"Country number {i}\",\n"))
        .collect();
    format!("COUNTRY_CODES = {{\n{body}}}\n")
}

#[test]
fn repeated_test_cases_warn_and_say_what_to_do() {
    let value = recap("tests/test_calc.py", PYTEST_BASE, &repeated_tests(), None);
    let lost = simple_lost(&value).unwrap_or_else(|| panic!("no SIMPLE loss: {value}"));
    assert_eq!(lost["severity"], "warn", "{lost}");
    assert_eq!(lost["metric"], "ast.entropy");
    let text = lost["text"].as_str().unwrap();
    assert!(text.contains("test file"), "{text}");
    assert!(text.contains("repetitive"), "{text}");
    assert!(
        lost["fix"].as_str().unwrap().contains("parametrize"),
        "{lost}"
    );
    assert_eq!(value["readiness"], "NEEDS_ATTENTION", "{value}");
}

#[test]
fn strict_still_blocks_repeated_test_cases() {
    let value = recap(
        "tests/test_calc.py",
        PYTEST_BASE,
        &repeated_tests(),
        Some("[pr_recap]\npreset = \"strict\"\n"),
    );
    let lost = simple_lost(&value).unwrap_or_else(|| panic!("no SIMPLE loss: {value}"));
    assert_eq!(lost["severity"], "block", "{lost}");
    assert_eq!(value["readiness"], "BLOCKED");
}

#[test]
fn a_growing_literal_table_warns_as_data() {
    let value = recap("app/codes.py", &dict(5), &dict(250), None);
    let lost = simple_lost(&value).unwrap_or_else(|| panic!("no SIMPLE loss: {value}"));
    assert_eq!(lost["severity"], "warn", "{lost}");
    assert!(
        lost["text"].as_str().unwrap().contains("data file"),
        "{lost}"
    );
    assert!(
        lost["fix"].as_str().unwrap().contains("data file"),
        "{lost}"
    );
}

#[test]
fn ordinary_code_that_turns_repetitive_still_blocks() {
    let base = "def route(request):\n    handler = request.get(\"handler\", \"index\")\n    \
        if handler == \"index\":\n        return {\"status\": 200, \"body\": \"home\"}\n    \
        return {\"status\": 404, \"body\": \"missing\"}\n";
    let handlers: String = (1..=60)
        .map(|i| format!("\n\ndef handle_{i}(request):\n    return {{\"status\": 200, \"body\": \"page\"}}\n"))
        .collect();
    let value = recap("app/router.py", base, &format!("{base}{handlers}"), None);
    let lost = simple_lost(&value).unwrap_or_else(|| panic!("no SIMPLE loss: {value}"));
    assert_eq!(lost["metric"], "ast.entropy");
    assert_eq!(lost["severity"], "block", "{lost}");
}

#[test]
fn a_test_file_that_grows_too_complex_still_blocks() {
    let branches: String = (1..=14)
        .map(|i| format!("    if x > {i}:\n        total += {i}\n"))
        .collect();
    let head = format!(
        "def test_simple():\n    assert 1 + 1 == 2\n\n\ndef test_branches(x=3):\n    total = 0\n\
         {branches}    assert total >= 0\n"
    );
    let value = recap(
        "tests/test_rules.py",
        "def test_simple():\n    assert 1 + 1 == 2\n",
        &head,
        None,
    );
    let lost = simple_lost(&value).unwrap_or_else(|| panic!("no SIMPLE loss: {value}"));
    assert_eq!(lost["metric"], "ast.max_function_complexity");
    assert_eq!(lost["severity"], "block", "{lost}");
}

#[test]
fn a_file_emptied_out_keeps_simple() {
    let base = "def clamp(value, low, high):\n    if value < low:\n        return low\n    \
        if value > high:\n        return high\n    return value\n";
    let value = recap("app/helpers.py", base, "", None);
    assert!(simple_lost(&value).is_none(), "{value}");
    assert_eq!(value["readiness"], "READY", "{value}");
}
