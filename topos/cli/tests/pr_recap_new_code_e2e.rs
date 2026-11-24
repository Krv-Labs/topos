//! New code meets the same complexity limit as changed code, driven through
//! the real binary against scratch repositories: a new file, or new logic
//! in a split child, that fails SIMPLE blocks under the recommended and
//! strict presets, while code moved unchanged out of another file does not.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

const ALPHA: &str = "def alpha(x):\n    if x:\n        return 1\n    return 0\n";

const BETA: &str = "def beta(x):\n    if x:\n        return 2\n    return 0\n";

/// A function of `branches` `if`s: cyclomatic complexity `branches + 1`.
fn branchy(name: &str, branches: usize) -> String {
    let body: String = (1..=branches)
        .map(|i| format!("    if x == {i}:\n        return {}\n", i * 7))
        .collect();
    format!("def {name}(x):\n{body}    return 0\n")
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn write(repo: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// A repository with `base` committed, then `head` written over it and
/// committed.
fn range(base: &[(&str, &str)], head: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.email", "recap@example.com"]);
    git(repo, &["config", "user.name", "Recap"]);
    write(repo, base);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "base"]);
    write(repo, head);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "head"]);
    dir
}

/// `pr-recap` over the last commit under `preset`: its exit code and JSON.
fn recap(repo: &Path, preset: &str) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args([
            "pr-recap",
            "--base",
            "HEAD~1",
            "--head",
            "HEAD",
            "--json",
            "--no-input",
            "--no-coupling",
            "--preset",
            preset,
        ])
        .current_dir(repo)
        .env("NO_COLOR", "1")
        .env("TOPOS_CACHE_INDEX_DIR", repo.join(".git/topos-cache-index"))
        .stdin(Stdio::null())
        .output()
        .expect("failed to spawn the topos binary under test");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), json)
}

fn findings<'a>(json: &'a Value, gate: &str) -> Vec<&'a Value> {
    json["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["gate"] == gate)
        .collect()
}

/// Blocked under both presets by one `new_file_pillar` block at `path` that
/// names `function`, its complexity, the limit and the fix.
fn assert_blocked_on(repo: &Path, path: &str, function: &str, complexity: usize) {
    for preset in ["recommended", "strict"] {
        let (code, json) = recap(repo, preset);
        assert_eq!(json["readiness"], "BLOCKED", "{preset}: {json:#}");
        assert_eq!(code, 1, "{preset}");
        let found = findings(&json, "new_file_pillar");
        assert_eq!(found.len(), 1, "{preset}: {json:#}");
        let finding = found[0];
        assert_eq!(finding["severity"], "block", "{preset}");
        assert_eq!(finding["path"], path, "{preset}");
        assert_eq!(finding["function"], function, "{preset}");
        let text = finding["text"].as_str().unwrap();
        assert!(
            text.contains(&format!(
                "ast.max_function_complexity is {complexity}, limit 10"
            )),
            "{preset}: {text}"
        );
        assert!(
            finding["fix"].as_str().unwrap().contains("helper"),
            "{preset}: {finding:#}"
        );
    }
}

fn assert_not_blocked(repo: &Path) {
    for preset in ["recommended", "strict"] {
        let (code, json) = recap(repo, preset);
        assert_ne!(json["readiness"], "BLOCKED", "{preset}: {json:#}");
        assert!(
            findings(&json, "new_file_pillar").is_empty(),
            "{preset}: {json:#}"
        );
        if preset == "recommended" {
            assert_eq!(code, 0, "{json:#}");
        }
    }
}

#[test]
fn a_new_file_with_a_function_over_the_limit_blocks() {
    let route = branchy("route", 11);
    let dir = range(&[("src/app.py", ALPHA)], &[("src/route.py", &route)]);
    assert_blocked_on(dir.path(), "src/route.py", "route", 12);
}

#[test]
fn a_new_file_within_the_limit_passes() {
    let route = branchy("route", 9);
    let dir = range(&[("src/app.py", ALPHA)], &[("src/route.py", &route)]);
    for preset in ["recommended", "strict"] {
        let (code, json) = recap(dir.path(), preset);
        assert_eq!(json["readiness"], "READY", "{preset}: {json:#}");
        assert_eq!(code, 0, "{preset}");
    }
}

/// Moving one small function out into a new module does not excuse new
/// logic written beside it.
#[test]
fn new_logic_in_a_split_child_blocks() {
    let route = branchy("route", 13);
    let dir = range(
        &[("src/big.py", &format!("{ALPHA}\n\n{BETA}"))],
        &[
            (
                "src/big.py",
                &format!("from helpers import beta\n\n\n{ALPHA}"),
            ),
            ("src/helpers.py", &format!("{BETA}\n\n{route}")),
        ],
    );
    let (_, json) = recap(dir.path(), "recommended");
    assert_eq!(
        json["clusters"][0]["children"][0]["path"], "src/helpers.py",
        "the repro must be a split: {json:#}"
    );
    assert_blocked_on(dir.path(), "src/helpers.py", "route", 14);
}

/// A function already over the limit, moved unchanged into a new module the
/// parent imports, is a refactor: its complexity predates the change.
#[test]
fn a_complex_function_moved_unchanged_into_a_split_child_passes() {
    let tangled = branchy("tangled", 13);
    let dir = range(
        &[("src/big.py", &format!("{ALPHA}\n\n{tangled}"))],
        &[
            (
                "src/big.py",
                &format!("from helpers import tangled\n\n\n{ALPHA}"),
            ),
            ("src/helpers.py", &tangled),
        ],
    );
    assert_not_blocked(dir.path());
}

/// The same move without the import is no split, but the ledger still
/// traces the function to the file it left.
#[test]
fn a_complex_function_moved_unchanged_into_a_new_file_passes() {
    let tangled = branchy("tangled", 13);
    let dir = range(
        &[("src/big.py", &format!("{ALPHA}\n\n{tangled}"))],
        &[("src/big.py", ALPHA), ("src/helpers.py", &tangled)],
    );
    assert_not_blocked(dir.path());
    let (_, json) = recap(dir.path(), "recommended");
    let moved = findings(&json, "moved_pillar");
    assert_eq!(moved.len(), 1, "{json:#}");
    assert_eq!(moved[0]["moved_from"], "src/big.py");
}

/// A function that grew past the limit while it moved is new logic.
#[test]
fn a_function_that_grew_past_the_limit_while_moving_blocks() {
    let small = branchy("tangled", 3);
    let grown = branchy("tangled", 11);
    let dir = range(
        &[("src/big.py", &format!("{ALPHA}\n\n{small}"))],
        &[
            (
                "src/big.py",
                &format!("from helpers import tangled\n\n\n{ALPHA}"),
            ),
            ("src/helpers.py", &grown),
        ],
    );
    assert_blocked_on(dir.path(), "src/helpers.py", "tangled", 12);
}
