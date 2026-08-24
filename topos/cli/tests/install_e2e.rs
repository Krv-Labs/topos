//! End-to-end verification of `topos install` / `topos uninstall` /
//! `topos status`, driven through the real binary against a scratch `$HOME`.
//!
//! The unit tests under `src/commands/install/` prove each module in isolation.
//! What they cannot prove is the property the whole rewrite exists for: that a
//! full install/uninstall round trip **leaves no trace**. The defect that
//! motivated the rewrite was invisible at unit level — every module did its own
//! job correctly, and uninstall still left 16 directories behind in a bare
//! `$HOME`, because nobody owned the question "which directories did we bring
//! into existence?". Only a test that snapshots an entire home directory before
//! and after can catch that, so that is the centerpiece here.
//!
//! Everything goes through the CLI surface and the filesystem: an integration
//! test cannot import `pub(crate)` internals, which is a feature — it means
//! these assertions hold against the binary a user actually runs, including its
//! argument parsing, its exit codes and its `--json` contract.
//!
//! Conventions, matching the unit tests:
//!
//! * Every test gets its own scratch `$HOME` named with both the process id and
//!   a per-test label. `cargo test` is threaded and the pid is shared, so two
//!   tests reusing a label would wipe each other's seeds.
//! * The child always runs with `stdin` on `/dev/null` and both output streams
//!   piped, so no stream is a tty and the interactivity gate resolves to
//!   `Headless` deterministically (see [`a_headless_uninstall_applies_without_a_prompt`]).
//! * `PATH` is pinned to a minimal value. Nothing in `install` shells out; the
//!   point is that `preferred_path_alias` finds no `topos` on `$PATH` on any
//!   developer machine, so the recorded `command` is deterministically the
//!   cargo target path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use toml_edit::DocumentMut;

// ---------------------------------------------------------------------------
// Seeds — foreign content that must survive an install/uninstall round trip.
// ---------------------------------------------------------------------------

/// A `~/.claude.json` shaped like a real one: a sibling MCP server that is not
/// ours, plus a top-level key Claude Code maintains itself.
///
/// `other`'s command is an absolute path that does not exist rather than a bare
/// name, because `points_at_topos` falls through to a `same_file` check that
/// resolves a relative command against the *child's* working directory — a bare
/// `"foo"` could in principle be mistaken for a duplicate topos registration.
const SEED_CLAUDE_JSON: &str = r#"{
  "numStartups": 42,
  "mcpServers": {
    "other": { "command": "/opt/vendor/bin/other-mcp", "args": ["serve"] }
  }
}
"#;

/// Codex's config is TOML edited through `toml_edit`; the comment is the part
/// a naive regex or a serialize-from-scratch implementation would destroy.
const SEED_CODEX_TOML: &str = r#"# my codex
model = "gpt-5-codex"

[tui]
theme = "dark"
"#;

/// `GEMINI.md` is shared with Gemini CLI and holds the user's own rules. topos
/// registers no MCP server into it, so it must come out byte-identical.
const SEED_GEMINI_MD: &str = "# my rules\n\n@import ./team-style.md\nUse tabs.\n";

/// A VS Code `mcp.json` with comments. topos refuses to rewrite it rather than
/// silently dropping the comments, so this file is a `Conflict` throughout.
const SEED_VSCODE_JSONC: &str = r#"{
  // my own servers, hand-maintained
  "servers": {
    "local": { "type": "stdio", "command": "/usr/bin/env" },
  }
}
"#;

// ---------------------------------------------------------------------------
// Harness: scratch homes, running the binary, reading its JSON.
// ---------------------------------------------------------------------------

/// A fresh, empty scratch `$HOME`. `label` must be unique per test.
fn scratch_home(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("topos-install-e2e-{label}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write `text` to `home/relative`, creating parent directories.
fn seed(home: &Path, relative: &str, text: &str) -> PathBuf {
    let path = home.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn mkdirs(home: &Path, relatives: &[&str]) {
    for relative in relatives {
        fs::create_dir_all(home.join(relative)).unwrap();
    }
}

/// The shared user-config root Claude Desktop and VS Code live under.
///
/// This is a `never_prune` member on every platform, which is why every test
/// that checks for directory leaks has to pre-create it — see
/// [`install_then_uninstall_leaves_no_file_and_no_directory_behind`].
fn support_root(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("AppData/Roaming")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        home.join(".config")
    }
}

/// One completed run of the real binary.
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    /// Panic unless the run exited with `expected`, quoting both streams — a
    /// bare `assert_eq!` on the code alone makes CLI failures unreadable.
    fn expect_code(self, expected: i32) -> Self {
        assert_eq!(
            self.code, expected,
            "unexpected exit code\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.stdout, self.stderr
        );
        self
    }
}

/// Run the binary under test against `home`.
///
/// `HOME` (and `USERPROFILE`, for the Windows branch of `paths::home_dir`) point
/// at the scratch directory, and `APPDATA` is removed so `paths::app_data`
/// derives from the profile instead of the developer's real roaming directory —
/// otherwise a Windows run would write into the tester's actual config.
fn topos(home: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("APPDATA")
        // Deterministic `$PATH`: no `topos` on it, so `resolve_binary_path`
        // records `current_exe()` — the cargo target path — verbatim.
        .env(
            "PATH",
            if cfg!(windows) {
                "C:\\Windows\\System32"
            } else {
                "/usr/bin:/bin"
            },
        )
        .env("NO_COLOR", "1")
        // No stream may be a tty, or the uninstall gate would resolve to
        // `Interactive` / `Ambiguous` depending on how `cargo test` was invoked.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn the topos binary under test");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `topos status --json`, parsed. Asserting on this rather than on the human
/// output keeps these tests from breaking every time a glyph or a sentence
/// changes.
fn status_json(home: &Path) -> Value {
    let run = topos(home, &["status", "--json"]).expect_code(0);
    serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("status --json emitted invalid JSON ({e}):\n{}", run.stdout))
}

/// One harness row out of `status --json`.
fn harness<'a>(status: &'a Value, id: &str) -> &'a Value {
    status["harnesses"]
        .as_array()
        .expect("status --json has no `harnesses` array")
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("no `{id}` row in status --json"))
}

fn state_of(status: &Value, id: &str) -> String {
    harness(status, id)["state"]
        .as_str()
        .expect("harness row has no `state`")
        .to_string()
}

fn config_of(status: &Value, id: &str) -> PathBuf {
    PathBuf::from(
        harness(status, id)["config"]
            .as_str()
            .expect("harness row has no `config`"),
    )
}

/// Every harness id in table order, so tests can iterate the whole set without
/// duplicating the table.
const IDS: [&str; 9] = [
    "claude",
    "claude-desktop",
    "codex",
    "gemini",
    "copilot",
    "cursor",
    "vscode",
    "antigravity",
    "pi",
];

// ---------------------------------------------------------------------------
// Tree snapshots.
// ---------------------------------------------------------------------------

/// Every file (with its bytes) and every directory under a root, keyed by
/// path relative to that root.
///
/// Directories are tracked separately and deliberately: the motivating defect
/// left *no* stray files behind, only empty directories, so a files-only
/// snapshot would have declared the broken build clean.
#[derive(Default, PartialEq, Eq)]
struct Tree {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
}

fn snapshot(root: &Path) -> Tree {
    let mut tree = Tree::default();
    walk(root, root, &mut tree);
    tree
}

fn walk(root: &Path, dir: &Path, tree: &mut Tree) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        // `file_type` from the directory entry does not follow symlinks, so a
        // symlinked config is recorded as the link it is rather than recursed
        // into.
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            tree.dirs.insert(relative);
            walk(root, &path, tree);
        } else {
            tree.files
                .insert(relative, fs::read(&path).unwrap_or_default());
        }
    }
}

/// Paths present in `after` but not in `before`, as displayable strings.
fn added<'a, T: Ord + 'a>(
    before: impl IntoIterator<Item = &'a T>,
    after: impl IntoIterator<Item = &'a T>,
) -> Vec<&'a T> {
    let before: BTreeSet<&T> = before.into_iter().collect();
    after
        .into_iter()
        .filter(|item| !before.contains(item))
        .collect()
}

// ---------------------------------------------------------------------------
// Reading what was written.
// ---------------------------------------------------------------------------

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&text(path))
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

/// The `command` and `args` topos recorded in a harness config, whichever of
/// the three artifact shapes that config uses.
fn recorded_entry(path: &Path) -> (String, Vec<String>) {
    if path.extension().is_some_and(|ext| ext == "toml") {
        let doc: DocumentMut = text(path).parse().expect("codex config no longer parses");
        let entry = &doc["mcp_servers"]["topos"];
        let args = entry["args"]
            .as_array()
            .expect("`args` must be an array")
            .iter()
            .map(|arg| arg.as_str().expect("`args` must be strings").to_string())
            .collect();
        return (
            entry["command"].as_str().expect("no `command`").to_string(),
            args,
        );
    }
    let value = json(path);
    // VS Code names the container `servers`; every other JSON client uses
    // `mcpServers`.
    let container = value
        .get("mcpServers")
        .or_else(|| value.get("servers"))
        .unwrap_or_else(|| panic!("{} has no server container", path.display()));
    let entry = &container["topos"];
    let args = entry["args"]
        .as_array()
        .expect("`args` must be an array")
        .iter()
        .map(|arg| arg.as_str().expect("`args` must be strings").to_string())
        .collect();
    (
        entry["command"].as_str().expect("no `command`").to_string(),
        args,
    )
}

/// Rewrite a JSON config's recorded `command` to the bare `"topos"` an earlier
/// draft wrote — the exact drift this rewrite exists to detect and repair.
fn break_command(path: &Path) {
    let mut value = json(path);
    value["mcpServers"]["topos"]["command"] = Value::String("topos".to_string());
    fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

// ---------------------------------------------------------------------------
// 1 + 2. Leave no trace, and leave foreign content alone.
// ---------------------------------------------------------------------------

#[test]
fn install_then_uninstall_leaves_no_file_and_no_directory_behind() {
    let home = scratch_home("no-trace");

    // Pre-create the directories a machine with these tools installed would
    // already have. Two of them are load-bearing rather than decorative:
    // `~/.local/state` and the shared support root are `never_prune` members,
    // so if install had to create them uninstall could never take them back —
    // by design, per the contract. Pre-creating them is what lets the leak
    // assertions below be exact rather than approximate.
    mkdirs(
        &home,
        &[
            ".claude",
            ".codex",
            ".gemini",
            ".cursor",
            ".local/state",
            support_root(&home)
                .strip_prefix(&home)
                .unwrap()
                .to_str()
                .unwrap(),
        ],
    );
    // Deliberately absent, so the pruning half of uninstall has something to
    // prove: topos must create each of these and then take it back.
    let must_be_pruned = [
        home.join(".copilot"),
        support_root(&home).join("Claude"),
        home.join(".gemini/config"),
        home.join(".local/state/topos"),
    ];
    for dir in &must_be_pruned {
        assert!(!dir.exists(), "{} must start absent", dir.display());
    }

    let claude = seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    let codex = seed(&home, ".codex/config.toml", SEED_CODEX_TOML);
    let gemini_md = seed(&home, ".gemini/GEMINI.md", SEED_GEMINI_MD);
    let vscode = seed(
        &home,
        support_root(&home)
            .join("Code/User/mcp.json")
            .strip_prefix(&home)
            .unwrap()
            .to_str()
            .unwrap(),
        SEED_VSCODE_JSONC,
    );

    let before = snapshot(&home);

    // The commented VS Code config is a conflict topos refuses to touch, so a
    // whole-set install reports failure. Asserting the code rather than
    // ignoring it means an unrelated second failure cannot hide in here.
    let install = topos(&home, &["install", "--all"]).expect_code(1);
    assert!(
        install.stdout.contains("comments"),
        "the VS Code conflict was not explained:\n{}",
        install.stdout
    );

    // Leave-no-trace passes vacuously if install did nothing, so prove it took
    // effect before undoing it.
    let after_install = status_json(&home);
    for id in IDS.iter().filter(|id| **id != "vscode") {
        assert_eq!(
            state_of(&after_install, id),
            "active",
            "{id} was not configured"
        );
    }
    assert_eq!(state_of(&after_install, "vscode"), "conflict");
    for dir in &must_be_pruned {
        assert!(dir.is_dir(), "install never created {}", dir.display());
    }

    topos(&home, &["uninstall", "--all", "--purge-backups"]).expect_code(0);

    let after = snapshot(&home);

    // (a) No file topos created may survive: emptied configs, the `install.json`
    // ledger, `.topos.backup` snapshots and any `.topos.tmp` leftovers.
    let leaked_files = added(before.files.keys(), after.files.keys());
    assert!(
        leaked_files.is_empty(),
        "uninstall left files behind: {leaked_files:?}"
    );

    // (b) The centerpiece. An empty directory is invisible to a files-only
    // check and to a casual `ls` of a real `$HOME`, which is exactly how the
    // draft shipped with 16 of them. Every directory install created has to be
    // recorded and pruned, child before parent.
    let leaked_dirs = added(before.dirs.iter(), after.dirs.iter());
    assert!(
        leaked_dirs.is_empty(),
        "uninstall left directories behind: {leaked_dirs:?}"
    );
    for dir in &must_be_pruned {
        assert!(
            !dir.exists(),
            "{} was created but never pruned",
            dir.display()
        );
    }

    // The mirror image: a directory that predated topos is not topos's to
    // remove, even when it is empty and even when topos wrote a file into it.
    // `~/.claude` is the purest case — topos never writes into it at all.
    for dir in [".claude", ".codex", ".gemini", ".cursor", ".local/state"] {
        assert!(
            home.join(dir).is_dir(),
            "{dir} was pruned but predated topos"
        );
    }
    assert!(
        support_root(&home).is_dir(),
        "the shared support root was pruned"
    );

    // (c) Files topos *did* rewrite must parse back to their prior logical
    // value. Not byte-identity: `serde_json` re-emits a compact seed as pretty
    // JSON, which `preserve_order` makes a formatting change rather than a
    // semantic one, so bytes are the wrong yardstick here.
    assert_eq!(
        json(&claude),
        serde_json::from_str::<Value>(SEED_CLAUDE_JSON).unwrap(),
        "~/.claude.json did not come back to its seeded value"
    );
    let codex_after = text(&codex);
    assert!(
        codex_after.contains("# my codex"),
        "the user's TOML comment was destroyed:\n{codex_after}"
    );
    let codex_doc: DocumentMut = codex_after.parse().expect("codex config no longer parses");
    assert!(
        codex_doc.get("mcp_servers").is_none(),
        "an emptied `[mcp_servers]` table was left behind:\n{codex_after}"
    );
    assert_eq!(codex_doc["model"].as_str(), Some("gpt-5-codex"));
    assert_eq!(codex_doc["tui"]["theme"].as_str(), Some("dark"));

    // (d) Files topos never writes into must be byte-identical. These are named
    // explicitly rather than inferred from the diff, so a file quietly dropped
    // from the seed set cannot make this pass by absence.
    assert_eq!(fs::read(&gemini_md).unwrap(), SEED_GEMINI_MD.as_bytes());
    assert_eq!(fs::read(&vscode).unwrap(), SEED_VSCODE_JSONC.as_bytes());

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 3. The recorded command is an absolute path.
// ---------------------------------------------------------------------------

#[test]
fn every_recorded_command_is_an_absolute_path_with_exactly_the_mcp_argument() {
    let home = scratch_home("absolute");
    topos(&home, &["install", "--all"]).expect_code(0);
    let status = status_json(&home);

    // GUI-launched macOS apps inherit only `/usr/bin:/bin:/usr/sbin:/sbin`, so a
    // bare `"topos"` cannot spawn from Claude Desktop, Cursor or Antigravity on
    // any install channel. That bare `"topos"` is the defect this rewrite fixes,
    // so every one of the eight entries is checked, not a representative one.
    for id in IDS {
        let config = config_of(&status, id);
        let (command, args) = recorded_entry(&config);
        assert!(
            Path::new(&command).is_absolute(),
            "{id} recorded a relative command: {command:?}"
        );
        assert!(
            Path::new(&command).is_file(),
            "{id} recorded a command that is not a file: {command:?}"
        );
        // A bare `topos` prints usage and exits, which the client reports as
        // `-32000: Connection closed`, so `args` may never be empty or wrong.
        assert_eq!(args, vec!["mcp".to_string()], "{id} recorded wrong args");
        // The binary under test lives in the cargo target directory and is
        // therefore not on `$PATH` — `preferred_path_alias` finds no alias and
        // `current_exe()` is recorded verbatim.
        assert_eq!(command, env!("CARGO_BIN_EXE_topos"), "{id}");
        assert_eq!(
            Value::String(command),
            status["binary"],
            "{id} disagrees with the binary status reports"
        );
    }
    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 4. Idempotency.
// ---------------------------------------------------------------------------

#[test]
fn a_second_install_reports_everything_active_and_writes_nothing() {
    let home = scratch_home("idempotent");
    seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    seed(&home, ".codex/config.toml", SEED_CODEX_TOML);

    topos(&home, &["install", "--all"]).expect_code(0);
    let after_first = snapshot(&home);

    let second = topos(&home, &["install", "--all"]).expect_code(0);
    assert!(
        second
            .stdout
            .contains("MCP server registered in ~/.claude.json"),
        "a settled entry was not reported as active:\n{}",
        second.stdout
    );

    // Byte-identical, because `Artifact::apply` returns `Ok(None)` for an
    // already-correct entry and never reaches a write. A rewrite here would be
    // harmless-looking and genuinely bad: it churns the user's file on every
    // run, and re-running the write path is what destroyed pristine backups in
    // the draft.
    assert!(
        after_first == snapshot(&home),
        "a second install rewrote files that were already correct"
    );
    assert_eq!(status_json(&home)["active"], 9);

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 5. Drift is reported, then repaired.
// ---------------------------------------------------------------------------

#[test]
fn a_drifted_command_is_reported_incomplete_and_healed_by_install() {
    let home = scratch_home("drift");
    seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    topos(&home, &["install", "claude"]).expect_code(0);
    assert_eq!(state_of(&status_json(&home), "claude"), "active");

    break_command(&home.join(".claude.json"));

    // `Incomplete`, never `Conflict`: the entry is still ours, it just points
    // somewhere that no longer resolves. The detail has to be populated because
    // status renders it as the reason plus a `topos install claude` instruction,
    // and "needs repair" with no reason is the silent failure mode.
    let drifted = status_json(&home);
    assert_eq!(state_of(&drifted, "claude"), "incomplete");
    let detail = harness(&drifted, "claude")["detail"]
        .as_str()
        .expect("a drifted entry must explain itself");
    assert!(detail.contains("topos"), "unhelpful drift detail: {detail}");

    topos(&home, &["install", "claude"]).expect_code(0);
    assert_eq!(state_of(&status_json(&home), "claude"), "active");
    let (command, _) = recorded_entry(&home.join(".claude.json"));
    assert!(
        Path::new(&command).is_absolute(),
        "repair left {command:?} relative"
    );

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 6. The backup stays pristine.
// ---------------------------------------------------------------------------

#[test]
fn repairing_drift_does_not_overwrite_the_pristine_pre_install_backup() {
    let home = scratch_home("backup");
    let claude = seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    let backup = home.join(".claude.json.topos.backup");

    topos(&home, &["install", "claude"]).expect_code(0);
    assert_eq!(
        fs::read(&backup).unwrap(),
        SEED_CLAUDE_JSON.as_bytes(),
        "the first install did not snapshot the user's original file"
    );

    break_command(&claude);
    topos(&home, &["install", "claude"]).expect_code(0);

    // The measured regression: because the draft decided to back up from
    // `path.is_file()`, the repair replaced the pristine snapshot with one that
    // already contained topos's own (stale) entry. Self-healing makes repair a
    // routine event, so that destroys the only copy of the pre-install file.
    let snapshot_after_repair = fs::read(&backup).unwrap();
    assert_eq!(
        snapshot_after_repair,
        SEED_CLAUDE_JSON.as_bytes(),
        "the repair clobbered the pristine backup"
    );
    assert!(
        !String::from_utf8_lossy(&snapshot_after_repair).contains("\"topos\""),
        "the backup captured topos's own entry"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn purging_backups_spares_the_harnesses_that_were_not_named() {
    let home = scratch_home("purge-scope");
    seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    seed(&home, ".codex/config.toml", SEED_CODEX_TOML);
    let claude_backup = home.join(".claude.json.topos.backup");
    let codex_backup = home.join(".codex/config.toml.topos.backup");

    topos(&home, &["install", "claude", "codex"]).expect_code(0);
    assert!(claude_backup.is_file() && codex_backup.is_file());

    topos(&home, &["uninstall", "codex", "--purge-backups"]).expect_code(0);

    // A backup is the only copy of a config as it stood before topos touched
    // it, so a scoped uninstall must not destroy the snapshot belonging to an
    // install the user is keeping.
    assert!(!codex_backup.exists(), "the named harness kept its backup");
    assert_eq!(
        fs::read(&claude_backup).unwrap(),
        SEED_CLAUDE_JSON.as_bytes(),
        "purging codex's backup also destroyed Claude Code's"
    );
    assert_eq!(state_of(&status_json(&home), "claude"), "active");

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 7. The interactivity gate — the Headless arm.
// ---------------------------------------------------------------------------

#[test]
fn a_headless_uninstall_applies_without_a_prompt() {
    let home = scratch_home("headless");
    seed(&home, ".claude.json", SEED_CLAUDE_JSON);
    topos(&home, &["install", "--all"]).expect_code(0);
    assert_eq!(status_json(&home)["active"], 9);

    // No `--yes`, no `--dry-run`, and no stream is a tty (see `topos`), so this
    // is the `Headless` arm: CI parity, apply. The `Ambiguous` arm — stderr
    // redirected while stdin or stdout is still a tty — needs a real pty and is
    // covered by the unit tests in `mod.rs` instead.
    let run = topos(&home, &["uninstall", "--all"]).expect_code(0);
    assert!(
        !run.stdout.contains("dry run"),
        "a headless uninstall previewed instead of applying:\n{}",
        run.stdout
    );

    // Exit 0 alone would also be satisfied by bailing out quietly, so assert the
    // removal actually happened.
    assert_eq!(status_json(&home)["active"], 0);

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 8. Residue is reported and never touched.
// ---------------------------------------------------------------------------

#[test]
fn residue_is_reported_by_status_and_never_modified_by_uninstall() {
    let home = scratch_home("residue");

    // Three distinct kinds: prose an earlier draft wrote into a file shared with
    // Copilot, an `@import` line in a file shared with Gemini CLI, and a
    // hand-made second registration under a key that is not `topos`.
    let copilot_md = seed(
        &home,
        ".copilot/copilot-instructions.md",
        "my notes\n<!-- topos:start -->\nblock\n<!-- topos:end -->\n",
    );
    let gemini_md = seed(
        &home,
        ".gemini/GEMINI.md",
        "# my rules\n@import ./topos-skill.md\nUse tabs.\n",
    );
    let cursor = seed(
        &home,
        ".cursor/mcp.json",
        "{\n  \"mcpServers\": {\n    \"topos-mcp\": { \"command\": \"topos\", \"args\": [\"mcp\"] }\n  }\n}\n",
    );

    let status = status_json(&home);
    let residue = status["residue"].as_array().expect("no `residue` array");
    let paths: BTreeSet<&str> = residue
        .iter()
        .map(|item| item["path"].as_str().expect("residue row has no `path`"))
        .collect();
    assert_eq!(residue.len(), 3, "unexpected residue rows: {residue:#?}");
    for expected in [&copilot_md, &gemini_md, &cursor] {
        assert!(
            paths.contains(expected.display().to_string().as_str()),
            "{} was not reported as residue",
            expected.display()
        );
    }
    for item in residue {
        assert!(!item["what"].as_str().unwrap_or_default().is_empty());
        assert!(!item["advice"].as_str().unwrap_or_default().is_empty());
    }

    // None of it is a topos-owned MCP entry, so uninstall must walk straight
    // past all three. `topos-mcp` in particular is the user's own registration:
    // reporting it is right, deleting it would be an installer editing config it
    // did not write.
    let before = snapshot(&home);
    topos(&home, &["uninstall", "--all"]).expect_code(0);
    let after = snapshot(&home);

    for path in [&copilot_md, &gemini_md, &cursor] {
        let relative = path.strip_prefix(&home).unwrap();
        assert_eq!(
            after.files.get(relative),
            before.files.get(relative),
            "{} was modified",
            path.display()
        );
    }
    // An uninstall with nothing of ours installed must also not bring the state
    // ledger — or its directories — into existence just to record an absence.
    assert!(
        before == after,
        "an uninstall with nothing to do changed the tree"
    );

    fs::remove_dir_all(&home).ok();
}

// ---------------------------------------------------------------------------
// 9. A commented VS Code config is refused, not clobbered.
// ---------------------------------------------------------------------------

#[test]
fn a_commented_vscode_config_is_refused_and_left_byte_identical() {
    let home = scratch_home("jsonc");
    let vscode = seed(
        &home,
        support_root(&home)
            .join("Code/User/mcp.json")
            .strip_prefix(&home)
            .unwrap()
            .to_str()
            .unwrap(),
        SEED_VSCODE_JSONC,
    );

    // Installed on its own rather than via `--all`, so the non-zero exit and the
    // reported reason can only have come from this harness.
    let run = topos(&home, &["install", "vscode"]).expect_code(1);
    assert!(
        run.stdout.contains("comments"),
        "the refusal did not say why:\n{}",
        run.stdout
    );
    // The conflict message carries the entry to paste, so the user does not have
    // to look it up.
    assert!(
        run.stdout.contains("stdio"),
        "no entry to paste:\n{}",
        run.stdout
    );

    assert_eq!(state_of(&status_json(&home), "vscode"), "conflict");
    // Rewriting this file through `serde_json` would silently delete the user's
    // comments and their trailing comma. Refusing is the whole point.
    assert_eq!(fs::read(&vscode).unwrap(), SEED_VSCODE_JSONC.as_bytes());

    fs::remove_dir_all(&home).ok();
}
