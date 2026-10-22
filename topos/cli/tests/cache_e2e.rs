//! End-to-end checks for the cross-repository cache inventory.
//!
//! No network and no GitNexus: real temporary Git repositories provide the
//! worktree registrations, and tiny `lbug` directories provide the index
//! evidence the status model looks for.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn topos(home: &Path, index: &Path, args: &[&str], cwd: &Path) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("TOPOS_CACHE_INDEX_DIR", index)
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env("NO_COLOR", "1")
        .output()
        .expect("running topos");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2020-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00Z")
        .output()
        .expect("running git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repo(dir: &Path) -> String {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "cache@example.com"]);
    git(dir, &["config", "user.name", "Cache Test"]);
    fs::write(dir.join("a.txt"), "hello\n").unwrap();
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "init"]);
    git(dir, &["rev-parse", "HEAD"])
}

fn graph(dir: &Path) {
    fs::create_dir_all(dir.join(".gitnexus/lbug")).unwrap();
    fs::write(dir.join(".gitnexus/lbug/data"), vec![0u8; 128]).unwrap();
}

fn add_pr_store(repo: &Path, pr: u64, sha: &str) -> PathBuf {
    let common = PathBuf::from(git(repo, &["rev-parse", "--absolute-git-dir"]));
    let parent = common.join(format!("topos-pr-{pr}"));
    for side in ["base", "head"] {
        let path = parent.join(side);
        git(
            repo,
            &[
                "worktree",
                "add",
                "--detach",
                "-q",
                path.to_str().unwrap(),
                sha,
            ],
        );
        graph(&path);
    }
    fs::write(
        parent.join("commits"),
        format!("{sha}\n{sha}\nelapsed_ms=1\n"),
    )
    .unwrap();
    fs::write(
        parent.join("topos-cache.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "topos.pr_store.v1",
            "layout": 1,
            "pr": pr,
            "common_git_dir": common.display().to_string(),
            "project_root": repo.display().to_string(),
            "created_at": 1.0,
            "generator": "gitnexus@test"
        }))
        .unwrap(),
    )
    .unwrap();
    parent
}

fn parse(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| panic!("{error}: {stdout}"))
}

#[test]
fn discovery_deduplicates_a_shared_slot_but_not_linked_worktree_graphs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    graph(&main);
    let linked = temp.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            linked.to_str().unwrap(),
            &sha,
        ],
    );
    graph(&linked);
    add_pr_store(&main, 7, &sha);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &[
            "cache",
            "discover",
            main.to_str().unwrap(),
            linked.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
        temp.path(),
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["schema"], "topos.cache.v1");
    assert_eq!(value["dry_run"], true);
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{stdout}");
    assert_eq!(
        entries.iter().filter(|entry| entry["kind"] == "pr").count(),
        1,
        "{stdout}"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["kind"] == "graph")
            .count(),
        2,
        "{stdout}"
    );
    let pr = entries.iter().find(|entry| entry["kind"] == "pr").unwrap();
    assert_eq!(pr["state"], "ready");
    assert_eq!(pr["managed"], true);
    assert!(!index.exists(), "dry-run created the inventory");
}

#[test]
fn status_does_not_refresh_last_used_or_rewrite_inventory() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    add_pr_store(&main, 3, &sha);

    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "discover", main.to_str().unwrap(), "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    let entries = fs::read_dir(index.join("entries"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let before = fs::read(&entries[0]).unwrap();
    let before_value: Value = serde_json::from_slice(&before).unwrap();
    assert!(before_value.get("last_used").is_none(), "{before_value}");

    let (code, stdout, stderr) = topos(&home, &index, &["cache", "status", "--json"], &main);
    assert_eq!(code, 0, "{stderr}");
    let after = fs::read(&entries[0]).unwrap();
    assert_eq!(before, after, "status rewrote inventory");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["state"], "ready");
    assert_eq!(value["entries"][0]["managed"], true);
    assert!(value["entries"][0].get("last_used").is_none());
}

#[test]
fn global_status_includes_scanned_and_inventory_locations_with_sizes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    graph(&main);
    add_pr_store(&main, 11, &sha);
    let other = temp.path().join("other");
    repo(&other);
    graph(&other);
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();

    let (code, _, stderr) = topos(
        &home,
        &index,
        &[
            "cache",
            "discover",
            main.to_str().unwrap(),
            other.to_str().unwrap(),
            "--json",
        ],
        &outside,
    );
    assert_eq!(code, 0, "{stderr}");

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "status", "--global", "--sizes", "--json"],
        &outside,
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["scope"], "global");
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(
        entries.iter().filter(|entry| entry["kind"] == "pr").count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["kind"] == "graph")
            .count(),
        2
    );
    for entry in entries {
        assert!(entry["estimated_bytes"].as_u64().unwrap() >= 128);
    }
    assert!(value["totals"]["estimated_bytes"].as_u64().unwrap() >= 256);
}

#[test]
fn non_git_graphs_and_immediate_repositories_are_discovered_without_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let parent = temp.path().join("workspace");
    fs::create_dir_all(parent.join(".gitnexus/lbug")).unwrap();
    let child = parent.join("child");
    repo(&child);
    graph(&child);
    let outside = temp.path().join("outside");
    repo(&outside);
    let link = parent.join("linked-outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "discover", parent.to_str().unwrap(), "--json"],
        temp.path(),
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{stdout}");
    assert!(entries.iter().all(|entry| entry["kind"] == "graph"));
    assert!(entries
        .iter()
        .all(|entry| !entry["path"].as_str().unwrap().contains("linked-outside")));
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn status_outside_a_repository_requires_global_scope() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let (code, _, stderr) = topos(&home, &index, &["cache", "status"], temp.path());
    assert_eq!(code, 1);
    assert!(stderr.contains("--global"), "{stderr}");
    assert!(!index.exists(), "failed status created inventory");

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "status", "--global", "--json"],
        temp.path(),
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["entries"].as_array().unwrap().len(), 0);
}
