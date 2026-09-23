//! End-to-end checks of how `topos pr-recap` behaves with no one to ask,
//! driven through the real binary against a scratch repository.
//!
//! The unit tests prove the prompt policy (`interaction::resolve`) and the
//! coupling plan in isolation. What they cannot prove is that the binary a
//! CI job runs never stops to ask: a prompt waiting on a closed stdin hangs
//! the job until its own timeout. So every run here is spawned with stdin
//! on `/dev/null`, output streams redirected to files, and a hard deadline
//! after which the child is killed and the test fails.
//!
//! Conventions:
//!
//! * No network. Pull requests resolve through a fake `gh` on `PATH`, and
//!   the graphs build through a fake `gitnexus`, so a developer's real
//!   GitNexus is never run by the suite.
//! * `CI` and `GH_PROMPT_DISABLED` are removed, so the runs rely on the
//!   missing terminal alone to take each question's default.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Longer than any honest run here takes; a run past it is waiting on input.
const DEADLINE: Duration = Duration::from_secs(60);

/// Text only the coupling question prints.
const PROMPT_MARKS: [&str; 3] = ["Build coupling graphs", "❯", "esc skips"];

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run the binary in `repo` with `bin` first on `PATH`, killing it at the
/// deadline.
fn topos(repo: &Path, bin: &Path, args: &[&str]) -> Run {
    topos_on_path(repo, &format!("{}:/usr/bin:/bin", bin.display()), args)
}

/// [`topos`] with an exact `PATH`.
fn topos_on_path(repo: &Path, path: &str, args: &[&str]) -> Run {
    // Beside the repository, so its `git status` stays clean.
    let scratch = repo.parent().expect("the repository has a parent");
    let out = scratch.join("topos.out");
    let err = scratch.join("topos.err");
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(args)
        .current_dir(repo)
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .env_remove("CI")
        .env_remove("GH_PROMPT_DISABLED")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap())
        .spawn()
        .expect("failed to spawn the topos binary under test");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("waiting on topos") {
            break status;
        }
        if started.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "topos {args:?} was still running after {DEADLINE:?}; stderr:\n{}",
                fs::read_to_string(&err).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Run {
        code: status.code().unwrap_or(-1),
        stdout: fs::read_to_string(&out).unwrap(),
        stderr: fs::read_to_string(&err).unwrap(),
    }
}

fn assert_no_prompt(run: &Run) {
    for mark in PROMPT_MARKS {
        assert!(!run.stderr.contains(mark), "{mark:?} in:\n{}", run.stderr);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A two-commit repository and an empty `bin` directory beside it.
fn scratch() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&bin).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "recap@example.com"]);
    git(&repo, &["config", "user.name", "Recap"]);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/a.py"), "def ready():\n    return 1\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    fs::write(
        repo.join("src/a.py"),
        "def ready(flag):\n    if flag:\n        return 1\n    return 2\n",
    )
    .unwrap();
    git(&repo, &["commit", "-qam", "head"]);
    (dir, repo, bin)
}

fn json(run: &Run) -> Value {
    serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}\nstderr:\n{}", run.stdout, run.stderr))
}

fn reason(value: &Value) -> &str {
    value["scope"]["coupling"]["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("no coupling reason: {value}"))
}

#[test]
fn yes_and_no_input_together_are_a_usage_error() {
    let (_keep, repo, bin) = scratch();
    let run = topos(&repo, &bin, &["pr-recap", "--yes", "--no-input"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert!(run.stderr.contains("cannot be used with"), "{}", run.stderr);
}

#[test]
fn a_range_without_a_pull_request_never_asks() {
    let (_keep, repo, bin) = scratch();
    let run = topos(
        &repo,
        &bin,
        &["pr-recap", "--base", "HEAD~1", "--head", "HEAD", "--json"],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_no_prompt(&run);
    assert_eq!(reason(&json(&run)), "no_pr");
}

/// With `gh` missing, a pull request number fails before any question.
/// `PATH` holds only `git`: CI images ship `gh` in `/usr/bin`.
#[cfg(unix)]
#[test]
fn an_unresolvable_pull_request_fails_fast_without_asking() {
    let (_keep, repo, bin) = scratch();
    std::os::unix::fs::symlink(which("git"), bin.join("git")).unwrap();
    let run = topos_on_path(
        &repo,
        &bin.display().to_string(),
        &["pr-recap", "7", "--json"],
    );
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_no_prompt(&run);
    assert!(run.stderr.contains("pull request 7"), "{}", run.stderr);
}

/// The first `name` on the test process's own `PATH`.
#[cfg(unix)]
fn which(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| panic!("{name} is not on PATH"))
}

/// Stand-ins for `gh` (one same-repository pull request) and `gitnexus`
/// (an empty store, as fast as a cached one).
#[cfg(unix)]
fn fake_tools(repo: &Path, bin: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let base = git(repo, &["rev-parse", "HEAD~1"]);
    let head = git(repo, &["rev-parse", "HEAD"]);
    let gh = format!(
        "#!/bin/sh\ncat <<'EOF'\n{{\"number\":7,\"baseRefName\":\"main\",\"headRefName\":\"feature\",\
         \"baseRefOid\":\"{base}\",\"headRefOid\":\"{head}\",\"isCrossRepository\":false}}\nEOF\n"
    );
    let gitnexus = "#!/bin/sh\n[ \"$1\" = analyze ] && mkdir -p .gitnexus\nexit 0\n";
    for (name, body) in [("gh", gh.as_str()), ("gitnexus", gitnexus)] {
        let path = bin.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// With no terminal the question takes its default, which builds: the
/// run neither blocks nor prints the prompt, then reuses the stores.
#[cfg(unix)]
#[test]
fn a_cold_store_builds_without_a_prompt_when_no_one_can_answer() {
    let (_keep, repo, bin) = scratch();
    fake_tools(&repo, &bin);

    let first = topos(&repo, &bin, &["pr-recap", "7", "--json"]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert_no_prompt(&first);
    let value = json(&first);
    assert_eq!(reason(&value), "built", "{value}");
    let commits = fs::read_to_string(repo.join(".git/topos-pr-7/commits")).unwrap();
    assert!(commits
        .lines()
        .nth(2)
        .is_some_and(|line| line.starts_with("elapsed_ms=")));

    let skipped = topos(&repo, &bin, &["pr-recap", "7", "--json", "--no-coupling"]);
    assert_eq!(skipped.code, 0, "{}", skipped.stderr);
    assert_eq!(reason(&json(&skipped)), "flag");

    let again = topos(&repo, &bin, &["pr-recap", "7", "--json"]);
    assert_eq!(again.code, 0, "{}", again.stderr);
    assert_no_prompt(&again);
    assert_eq!(reason(&json(&again)), "cached");
}
