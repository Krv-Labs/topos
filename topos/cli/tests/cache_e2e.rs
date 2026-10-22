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
    let outside_bytes = value["totals"]["estimated_bytes"].as_u64().unwrap();
    assert!(outside_bytes >= 256);

    // The same inventory records must merge with the current-project scan,
    // not appear beside it with a second copy and a doubled size total.
    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "status", "--global", "--sizes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    let entries = value["entries"].as_array().unwrap();
    let mut paths = std::collections::BTreeSet::new();
    for entry in entries {
        assert!(
            paths.insert(entry["path"].as_str().unwrap()),
            "duplicate status entry: {stdout}"
        );
    }
    let main_graph = fs::canonicalize(&main)
        .unwrap()
        .join(".gitnexus")
        .display()
        .to_string();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["kind"] == "graph" && entry["path"] == main_graph)
            .count(),
        1,
        "the current project's graph was counted twice: {stdout}"
    );
    assert_eq!(
        entries.iter().filter(|entry| entry["kind"] == "pr").count(),
        1,
        "{stdout}"
    );
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

#[test]
fn an_empty_repository_is_a_successful_empty_status() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let empty = temp.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q"]);

    let (code, stdout, stderr) = topos(&home, &index, &["cache", "status", "--json"], &empty);

    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["scope"], "project");
    assert_eq!(value["entries"].as_array().unwrap().len(), 0);
}

#[test]
fn discovery_reports_an_unusable_inventory_location() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    fs::write(&index, "not a directory").unwrap();
    let main = temp.path().join("main");
    repo(&main);
    graph(&main);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "discover", main.to_str().unwrap(), "--json"],
        &main,
    );

    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics.iter().any(|line| line
            .as_str()
            .is_some_and(|line| line.contains("cache inventory"))),
        "{stdout}"
    );
}

fn common_dir(repo: &Path) -> PathBuf {
    PathBuf::from(git(repo, &["rev-parse", "--absolute-git-dir"]))
}

fn lease_path(repo: &Path, pr: u64) -> PathBuf {
    common_dir(repo).join(format!("topos-pr-{pr}.lease"))
}

fn inventory_entries(index: &Path) -> Vec<PathBuf> {
    match fs::read_dir(index.join("entries")) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("reading inventory: {error}"),
    }
}

#[test]
fn clean_dry_run_reports_without_adopting_or_deleting() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let store = add_pr_store(&main, 5, &sha);
    fs::remove_file(store.join("topos-cache.json")).unwrap();

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "5", "--dry-run", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["schema"], "topos.cache.v1");
    assert_eq!(value["command"], "clean");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["entries"][0]["action"], "would-remove", "{stdout}");
    assert!(store.exists(), "dry-run removed the store");
    assert!(
        !store.join("topos-cache.json").exists(),
        "dry-run adopted the legacy store"
    );
    assert!(
        !store.join("topos-delete.json").exists(),
        "dry-run wrote a deletion marker"
    );
    assert!(git(&main, &["worktree", "list", "--porcelain"]).contains(store.to_str().unwrap()));
}

#[test]
fn clean_json_refuses_to_prompt_without_yes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let store = add_pr_store(&main, 10, &sha);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "10", "--json"],
        &main,
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("--yes"), "{stderr}");
    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(store.exists(), "refused prompt still removed the store");
}

#[test]
fn clean_removes_a_managed_store_registrations_and_inventory() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let store = add_pr_store(&main, 6, &sha);

    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "discover", main.to_str().unwrap(), "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(inventory_entries(&index).len(), 1);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "6", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}\n{stdout}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["action"], "removed", "{stdout}");
    assert!(!store.exists(), "managed store survived cleanup");
    assert!(
        !git(&main, &["worktree", "list", "--porcelain"]).contains(store.to_str().unwrap()),
        "cleanup left a worktree registration"
    );
    assert!(
        inventory_entries(&index).is_empty(),
        "inventory record survived"
    );
    assert!(
        lease_path(&main, 6).exists(),
        "cleanup removed the PR lease"
    );
}

#[test]
fn status_reports_an_active_pr_lease() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    add_pr_store(&main, 31, &sha);

    let lease = fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lease_path(&main, 31))
        .unwrap();
    lease
        .try_lock()
        .unwrap_or_else(|error| panic!("taking test lease: {error}"));

    let (code, stdout, stderr) = topos(&home, &index, &["cache", "status", "--json"], &main);

    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["state"], "in-use", "{stdout}");
    assert!(value["entries"][0]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().is_some_and(|line| line.contains("lease"))));
}

#[test]
fn clean_refuses_user_content_and_keeps_the_store_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let store = add_pr_store(&main, 7, &sha);
    fs::write(store.join("base/user.txt"), "mine\n").unwrap();

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "7", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 1, "{stderr}\n{stdout}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["action"], "skipped", "{stdout}");
    assert!(value["totals"]["unsafe_skipped"].as_u64().unwrap() >= 1);
    assert!(store.exists(), "unsafe store was removed");
    assert_eq!(
        fs::read_to_string(store.join("base/user.txt")).unwrap(),
        "mine\n"
    );
    assert!(git(&main, &["worktree", "list", "--porcelain"]).contains(store.to_str().unwrap()));
}

#[test]
fn clean_refuses_tracked_gitnexus_content() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    repo(&main);
    fs::create_dir_all(main.join(".gitnexus")).unwrap();
    fs::write(main.join(".gitnexus/note.txt"), "tracked\n").unwrap();
    git(&main, &["add", ".gitnexus/note.txt"]);
    git(&main, &["commit", "-qm", "track graph note"]);
    let sha = git(&main, &["rev-parse", "HEAD"]);
    let store = add_pr_store(&main, 19, &sha);
    fs::write(store.join("base/.gitnexus/note.txt"), "edited\n").unwrap();

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "19", "--yes", "--json"],
        &main,
    );

    assert_eq!(code, 1, "{stderr}\n{stdout}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["action"], "skipped", "{stdout}");
    assert!(store.exists(), "tracked graph content authorized deletion");
    assert_eq!(
        fs::read_to_string(store.join("base/.gitnexus/note.txt")).unwrap(),
        "edited\n"
    );
}

#[test]
fn clean_adopts_a_valid_legacy_store_but_refuses_a_foreign_marker() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();

    let legacy_repo = temp.path().join("legacy");
    let legacy_sha = repo(&legacy_repo);
    let legacy = add_pr_store(&legacy_repo, 8, &legacy_sha);
    fs::remove_file(legacy.join("topos-cache.json")).unwrap();

    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "8", "--yes", "--json"],
        &legacy_repo,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !legacy.exists(),
        "valid legacy store was not adopted and removed"
    );

    let foreign_repo = temp.path().join("foreign");
    let foreign_sha = repo(&foreign_repo);
    let foreign = add_pr_store(&foreign_repo, 9, &foreign_sha);
    let marker_path = foreign.join("topos-cache.json");
    let mut marker: Value = serde_json::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
    marker["common_git_dir"] = Value::String(temp.path().join("elsewhere").display().to_string());
    fs::write(&marker_path, serde_json::to_vec_pretty(&marker).unwrap()).unwrap();

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "9", "--yes", "--json"],
        &foreign_repo,
    );
    assert_eq!(code, 1, "{stderr}\n{stdout}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["action"], "skipped", "{stdout}");
    assert!(foreign.exists(), "foreign marker authorized deletion");
}

#[test]
fn clean_all_prs_continues_around_a_busy_lease() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let busy = add_pr_store(&main, 30, &sha);
    let free = add_pr_store(&main, 31, &sha);

    let lease = fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lease_path(&main, 30))
        .unwrap();
    lease
        .try_lock()
        .unwrap_or_else(|error| panic!("taking test lease: {error}"));

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--all-prs", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 1, "{stderr}\n{stdout}");
    let value = parse(&stdout);
    let entries = value["entries"].as_array().unwrap();
    let busy_entry = entries.iter().find(|entry| entry["pr"] == 30).unwrap();
    assert_eq!(busy_entry["action"], "skipped", "{stdout}");
    assert_eq!(value["totals"]["busy"], 1, "{stdout}");
    assert!(busy.exists(), "busy store was removed");
    assert!(!free.exists(), "cleanup stopped before the unrelated store");

    drop(lease);
    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "30", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!busy.exists(), "released store was not retryable");
}

#[test]
fn clean_of_a_missing_pr_is_successful_and_reports_none() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    repo(&main);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &["cache", "clean", "--pr", "999", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["entries"][0]["action"], "none", "{stdout}");
}

#[cfg(unix)]
fn age_tree(path: &Path, when: std::time::SystemTime) {
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).unwrap().flatten() {
            age_tree(&entry.path(), when);
        }
    }
    let file = fs::File::open(path).unwrap();
    file.set_times(fs::FileTimes::new().set_modified(when))
        .unwrap();
}

#[cfg(unix)]
#[test]
fn prune_uses_a_twenty_four_hour_grace_and_predicts_without_deleting() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let index = temp.path().join("index");
    fs::create_dir_all(&home).unwrap();
    let main = temp.path().join("main");
    let sha = repo(&main);
    let old = add_pr_store(&main, 40, &sha);
    let young = add_pr_store(&main, 41, &sha);
    let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    age_tree(&old, long_ago);

    let (code, stdout, stderr) = topos(
        &home,
        &index,
        &[
            "cache",
            "prune",
            "--older-than",
            "1h",
            "--dry-run",
            "--json",
        ],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    let value = parse(&stdout);
    assert_eq!(value["command"], "prune");
    assert_eq!(value["dry_run"], true);
    let entries = value["entries"].as_array().unwrap();
    let old_entry = entries.iter().find(|entry| entry["pr"] == 40).unwrap();
    assert_eq!(old_entry["action"], "would-remove", "{stdout}");
    assert!(old.exists(), "prune dry-run deleted the store");

    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "prune", "--older-than", "1h", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!old.exists(), "old store survived prune");
    assert!(
        young.exists(),
        "prune ignored the 24-hour grace period for a young store"
    );

    age_tree(&young, long_ago);
    let (code, _, stderr) = topos(
        &home,
        &index,
        &["cache", "prune", "--older-than", "1h", "--yes", "--json"],
        &main,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!young.exists(), "aged store survived a second prune");
}
