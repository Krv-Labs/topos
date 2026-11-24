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
const PROMPT_MARKS: [&str; 3] = ["Build coupling graphs", "❯", "esc skip"];

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
        .env("TOPOS_CACHE_INDEX_DIR", scratch.join("cache-index"))
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

const CLEAN: &str = "def ready():\n    return 1\n";
const SHELL: &str = "import os\n\ndef run(cmd):\n    os.system(cmd)\n";
const SELF_WAIVER: &str =
    "[[pr_recap.waive]]\ngate = \"pillar_lost\"\npath = \"src/**\"\nreason = \"trust me\"\n";
const ADVISORY: &str = "[pr_recap]\nfail_on = \"never\"\n";
const ALLOW_SHELL: &str = "[[secure.allow]]\npattern = \"os.system\"\nreason = \"trust me\"\n";

/// A repository whose base commit holds `base` and whose head commit
/// writes `head` over it, with an empty `bin` directory beside it.
fn range(base: &[(&str, &str)], head: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    let bin = dir.path().join("bin");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(&bin).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "recap@example.com"]);
    git(&repo, &["config", "user.name", "Recap"]);
    for (message, files) in [("base", base), ("head", head)] {
        for (path, body) in files {
            let full = repo.join(path);
            fs::create_dir_all(full.parent().expect("a file has a parent")).unwrap();
            fs::write(full, body).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", message]);
    }
    (dir, repo, bin)
}

fn committed_range(repo: &Path, bin: &Path, extra: &[&str]) -> Run {
    let mut args = vec!["pr-recap", "--base", "HEAD~1", "--head", "HEAD", "--json"];
    args.extend(extra);
    topos(repo, bin, &args)
}

fn gates(value: &Value) -> Vec<&str> {
    value["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding.get("waived").is_none())
        .filter_map(|finding| finding["gate"].as_str())
        .collect()
}

#[test]
fn a_waiver_the_change_adds_for_its_own_regression_does_not_pass_the_check() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/a.py", SHELL), (".topos.toml", SELF_WAIVER)],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(value["check"], "fail");
    assert!(gates(&value).contains(&"pillar_lost"), "{value:#}");
    assert_eq!(
        value["gate"]["unapplied"],
        serde_json::json!(["waiver added: pillar_lost on src/**"])
    );
    assert!(value.get("waivers").is_none(), "{value:#}");
    assert!(
        run.stderr.contains("these edits were not applied"),
        "{}",
        run.stderr
    );
    let card = topos(
        &repo,
        &bin,
        &["pr-recap", "--base", "HEAD~1", "--head", "HEAD"],
    );
    assert_eq!(card.code, 1, "{}", card.stdout);
    assert!(
        card.stdout.contains("waiver added: pillar_lost on src/**"),
        "{}",
        card.stdout
    );
    let comment = topos(
        &repo,
        &bin,
        &[
            "pr-recap", "--base", "HEAD~1", "--head", "HEAD", "--format", "github",
        ],
    );
    assert!(comment.stdout.contains("**Config:**"), "{}", comment.stdout);
}

/// The other half of the rule: a waiver already on the base still counts.
#[test]
fn a_waiver_on_the_base_revision_still_waives() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN), (".topos.toml", SELF_WAIVER)],
        &[("src/a.py", SHELL)],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(value["waivers"][0]["status"], "used");
    assert!(value["gate"].get("unapplied").is_none(), "{value:#}");
}

/// A flag is the caller's, not the change's: it still wins over the base.
#[test]
fn an_explicit_flag_still_overrides_the_base_policy() {
    let (_keep, repo, bin) = range(&[("src/a.py", CLEAN)], &[("src/a.py", SHELL)]);
    let run = committed_range(&repo, &bin, &["--advisory"]);
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(value["readiness"], "BLOCKED");
}

/// With no arguments the base is HEAD: an uncommitted waiver does not
/// clear an uncommitted regression.
#[test]
fn uncommitted_config_edits_do_not_judge_the_worktree() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/a.py", CLEAN), ("b.txt", "")],
    );
    fs::write(repo.join("src/a.py"), SHELL).unwrap();
    fs::write(repo.join(".topos.toml"), SELF_WAIVER).unwrap();
    let run = topos(&repo, &bin, &["pr-recap", "--json"]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(
        value["gate"]["unapplied"],
        serde_json::json!(["waiver added: pillar_lost on src/**"])
    );
}

#[test]
fn fail_on_never_set_by_the_change_does_not_pass_the_check() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/a.py", SHELL), (".topos.toml", ADVISORY)],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(value["gate"]["fail_on"], "block");
    assert_eq!(
        value["gate"]["unapplied"],
        serde_json::json!(["fail_on: block → never"])
    );
}

#[test]
fn an_allowlist_entry_the_change_adds_does_not_clear_its_own_dangerous_call() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/a.py", SHELL), (".topos.toml", ALLOW_SHELL)],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert!(gates(&value).contains(&"pillar_lost"), "{value:#}");
}

#[test]
fn a_file_that_stops_parsing_fails_the_check() {
    let (_keep, repo, bin) = range(&[("src/a.py", CLEAN)], &[("src/a.py", "def broken(:\n")]);
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(value["readiness"], "BLOCKED");
    let finding = &value["findings"][0];
    assert_eq!(finding["gate"], "pillar_lost");
    assert_eq!(finding["severity"], "block");
    assert!(
        finding["text"]
            .as_str()
            .is_some_and(|text| text.contains("no longer parses at head")),
        "{finding:#}"
    );
}

const BROKEN: &str = "def broken(:\n";

#[test]
fn an_added_file_that_does_not_parse_fails_the_check() {
    let (_keep, repo, bin) = range(&[("src/a.py", CLEAN)], &[("src/new.py", BROKEN)]);
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(value["readiness"], "BLOCKED");
    let finding = &value["findings"][0];
    assert_eq!(finding["gate"], "new_file_slop");
    assert_eq!(finding["severity"], "block");
    assert_eq!(finding["path"], "src/new.py");
    assert!(
        finding["text"]
            .as_str()
            .is_some_and(|text| text.contains("does not parse")),
        "{finding:#}"
    );
    for preset in ["relaxed", "recommended", "strict"] {
        let run = committed_range(&repo, &bin, &["--preset", preset]);
        assert_eq!(run.code, 1, "{preset}: {}", run.stdout);
    }
}

/// Files the recap does not score are not judged for parsing either.
#[test]
fn added_files_that_are_skipped_raise_nothing_when_they_do_not_parse() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[
            ("notes.txt", BROKEN),
            ("src/app.min.js", "function ( {"),
            ("src/data.bundle.js", "function ( {"),
        ],
    );
    fs::create_dir_all(repo.join("node_modules/pkg")).unwrap();
    fs::write(repo.join("node_modules/pkg/index.js"), "function ( {").unwrap();
    git(&repo, &["add", "-f", "."]);
    git(&repo, &["commit", "-qm", "vendored"]);
    let run = topos(
        &repo,
        &bin,
        &["pr-recap", "--base", "HEAD~2", "--head", "HEAD", "--json"],
    );
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(value["findings"], serde_json::json!([]));
    assert_eq!(value["scope"]["files_scored"], 0);
    assert_eq!(value["scope"]["files_skipped"], 4);
}

const GENERATED: &str = "# Code generated by protoc. DO NOT EDIT.\n";

fn skipped<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    value["skipped"]
        .as_array()
        .expect("skipped")
        .iter()
        .find(|file| file["path"] == path)
        .and_then(|file| file["reason"].as_str())
}

/// Parser fixtures are broken on purpose. `topos evaluate -r` never scores
/// them, so the recap must not block on them either.
#[test]
fn unparseable_fixtures_are_skipped_as_project_discovery_skips_them() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[
            ("tests/fixtures/broken.py", BROKEN),
            ("pkg/testdata/broken.py", BROKEN),
            ("ui/__snapshots__/broken.py", BROKEN),
            ("htmlcov/broken.py", BROKEN),
        ],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(value["findings"], serde_json::json!([]));
    assert_eq!(value["scope"]["files_skipped"], 4, "{value:#}");
}

#[test]
fn a_path_the_base_toposignore_lists_is_skipped() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN), (".toposignore", "samples/\n")],
        &[("samples/broken.py", BROKEN)],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(
        skipped(&value, "samples/broken.py"),
        Some("ignored by .toposignore at base")
    );
}

/// `.toposignore` is policy: a change cannot hide its own file with it.
#[test]
fn a_toposignore_entry_the_change_adds_does_not_hide_its_own_file() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/new.py", BROKEN), (".toposignore", "src/\n")],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(value["findings"][0]["gate"], "new_file_slop");
    assert_eq!(
        value["gate"]["unapplied"],
        serde_json::json!([".toposignore pattern added: src"])
    );
}

/// A generated-code header counts only when the base already carries it.
#[test]
fn a_generated_header_skips_a_file_only_when_the_base_has_it() {
    let generated = format!("{GENERATED}{CLEAN}");
    let (_keep, repo, bin) = range(
        &[("src/gen.py", generated.as_str()), ("src/a.py", CLEAN)],
        &[
            ("src/gen.py", &format!("{GENERATED}{BROKEN}")),
            ("src/a.py", &format!("{GENERATED}{SHELL}")),
        ],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(
        skipped(&value, "src/gen.py"),
        Some("generated-code header at base and head"),
        "{value:#}"
    );
    assert_eq!(skipped(&value, "src/a.py"), None);
    assert_eq!(run.code, 1, "a header added by the change hides nothing");
    assert_eq!(value["findings"][0]["path"], "src/a.py");

    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[("src/new.py", &format!("{GENERATED}{BROKEN}"))],
    );
    let run = committed_range(&repo, &bin, &[]);
    assert_eq!(
        run.code, 1,
        "an added file has no base header: {}",
        run.stdout
    );
}

/// One more commit on top of [`range`]'s two, made by `change`, so the
/// range under review is `HEAD~1..HEAD` with moves and deletions in it.
fn then(repo: &Path, change: impl FnOnce()) {
    change();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "change"]);
}

fn first_finding(value: &Value) -> (&str, &str) {
    let finding = &value["findings"][0];
    (
        finding["gate"].as_str().unwrap_or(""),
        finding["path"].as_str().unwrap_or(""),
    )
}

/// A generated file whose header the change removes is hand-written code
/// nobody reviewed: it is judged as a new file, in place or renamed.
#[test]
fn a_generated_file_that_loses_its_header_is_judged_as_new() {
    let generated = format!("{GENERATED}{CLEAN}");
    let (_keep, repo, bin) = range(
        &[("README.md", "# hi\n")],
        &[("src/gen.py", generated.as_str())],
    );
    then(&repo, || fs::write(repo.join("src/gen.py"), SHELL).unwrap());
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(first_finding(&value), ("new_file_insecure", "src/gen.py"));

    let (_keep, repo, bin) = range(
        &[("README.md", "# hi\n")],
        &[("src/gen.py", generated.as_str())],
    );
    then(&repo, || {
        git(&repo, &["mv", "src/gen.py", "src/handwritten.py"]);
        fs::write(repo.join("src/handwritten.py"), SHELL).unwrap();
    });
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 1, "{value:#}");
    assert_eq!(
        first_finding(&value),
        ("new_file_insecure", "src/handwritten.py")
    );
}

/// A file moved out of a skipped place was never reviewed at base, so its
/// old content is no baseline: it is judged as a new file.
#[test]
fn a_file_promoted_out_of_a_skipped_path_is_judged_as_new() {
    for (from, to) in [
        ("fixtures/evil.py", "src/evil.py"),
        ("notes/evil.txt", "src/evil.py"),
        ("samples/evil.py", "src/evil.py"),
    ] {
        let (_keep, repo, bin) = range(
            &[("README.md", "# hi\n"), (".toposignore", "samples/\n")],
            &[(from, SHELL)],
        );
        then(&repo, || {
            fs::create_dir_all(repo.join("src")).unwrap();
            git(&repo, &["mv", from, to]);
        });
        let run = committed_range(&repo, &bin, &[]);
        let value = json(&run);
        assert_eq!(run.code, 1, "{from}: {value:#}");
        assert_eq!(first_finding(&value), ("new_file_insecure", to), "{from}");
    }
}

/// The author picks the path, so source that was skipped must be named
/// where a maintainer reads, without failing the check.
#[test]
fn skipped_source_files_are_named_in_every_output() {
    let (_keep, repo, bin) = range(
        &[("src/a.py", CLEAN)],
        &[
            ("tests/fixtures/evil.py", SHELL),
            ("vendor/lib.py", SHELL),
            ("README.md", "# hi\n"),
        ],
    );
    let run = committed_range(&repo, &bin, &[]);
    let value = json(&run);
    assert_eq!(run.code, 0, "{value:#}");
    assert_eq!(value["scope"]["files_skipped"], 3);
    assert_eq!(value["scope"]["files_unreviewed"], 2);
    let source = |path: &str| {
        value["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == path)
            .map(|file| file["source"] == true)
    };
    assert_eq!(source("tests/fixtures/evil.py"), Some(true));
    assert_eq!(source("README.md"), Some(false));

    let range_args = ["pr-recap", "--base", "HEAD~1", "--head", "HEAD"];
    let card = topos(&repo, &bin, &range_args);
    assert!(
        card.stdout.contains("2 source files not reviewed")
            && card.stdout.contains("tests/fixtures/evil.py"),
        "{}",
        card.stdout
    );
    let mut github = range_args.to_vec();
    github.extend(["--format", "github"]);
    let comment = topos(&repo, &bin, &github);
    assert!(
        comment.stdout.contains("**Not reviewed**") && comment.stdout.contains("`vendor/lib.py`"),
        "{}",
        comment.stdout
    );
}

/// The remedy for a file Topos cannot parse is on the card, both halves.
#[test]
fn an_unparseable_file_says_how_to_fix_or_exempt_it() {
    let (_keep, repo, bin) = range(&[("src/a.py", CLEAN)], &[("src/new.py", BROKEN)]);
    for format in ["card", "github"] {
        let run = topos(
            &repo,
            &bin,
            &[
                "pr-recap", "--base", "HEAD~1", "--head", "HEAD", "--format", format,
            ],
        );
        assert!(
            run.stdout.contains("fix the syntax error")
                && run.stdout.contains(".toposignore")
                && run.stdout.contains("base branch"),
            "{format}: {}",
            run.stdout
        );
    }
}

#[test]
fn a_split_child_says_what_attached_it_through_the_binary() {
    let alpha = "def alpha(x):\n    if x:\n        return 1\n    return 0\n";
    let moved = "def beta(x):\n    if x:\n        return 2\n    return 0\n\n\ndef gamma(x):\n    if x:\n        return 3\n    return 0\n";
    let (_keep, repo, bin) = range(
        &[("src/big.py", &format!("{alpha}\n\n{moved}"))],
        &[
            (
                "src/big.py",
                &format!("from helpers import beta, gamma\n\n\n{alpha}"),
            ),
            ("src/helpers.py", moved),
        ],
    );
    let value = json(&committed_range(&repo, &bin, &[]));
    let child = &value["clusters"][0]["children"][0];
    assert_eq!(child["path"], "src/helpers.py", "{value:#}");
    assert_eq!(child["attached_by"], "import_line");
}

/// The base branch moved on after the fork. Its tip is the policy, and
/// what it changed is not charged to the branch as the branch's own edit.
#[test]
fn the_base_branch_tip_judges_a_branch_that_forked_before_it() {
    let (_keep, repo, bin) = range(&[("README.md", "# hi\n")], &[("src/a.py", CLEAN)]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    then(&repo, || fs::write(repo.join("src/a.py"), SHELL).unwrap());
    git(&repo, &["checkout", "-q", "main"]);
    then(&repo, || {
        fs::write(repo.join(".topos.toml"), SELF_WAIVER).unwrap()
    });

    let args = ["pr-recap", "--base", "main", "--head", "feature", "--json"];
    let waived = topos(&repo, &bin, &args);
    let value = json(&waived);
    assert_eq!(waived.code, 0, "the tip's waiver applies: {value:#}");
    assert_eq!(value["waivers"][0]["status"], "used");
    assert!(value["gate"].get("unapplied").is_none(), "{value:#}");

    then(&repo, || fs::remove_file(repo.join(".topos.toml")).unwrap());
    let blocked = topos(&repo, &bin, &args);
    let value = json(&blocked);
    assert_eq!(blocked.code, 1, "the tip dropped the waiver: {value:#}");
    assert!(value["gate"].get("unapplied").is_none(), "{value:#}");
}
