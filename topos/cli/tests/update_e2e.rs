//! `topos update` against the real binary, with no network.
//!
//! A fake `curl` on `PATH` drives version lookups and release asset downloads, and it goes through `curl` with
//! `-w '%{url_effective}'`; a script that prints a release redirect URL makes
//! the check deterministic and lets the tests drive both the "newer release
//! exists" and "you are already current" branches without a release ever
//! being cut.
//!
//! The prompt gate is asserted rather than assumed: every non-TTY run here
//! must leave the filesystem alone, because an agent or a CI job has nobody to
//! answer an install prompt. A regression that made `topos update` download
//! unprompted would run a real `install.sh` in CI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Longer than any honest run here takes; a run past it is waiting on input.
const DEADLINE: Duration = Duration::from_secs(30);

/// Text only the interactive prompt prints. A non-TTY run must never contain
/// any of it.
const PROMPT_MARKS: [&str; 3] = ["↑↓ move", "esc skip", "Continue with current version"];

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// A scratch home, a scratch `PATH`, and a fake `curl` on it.
struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    /// A fake `curl` that answers with `tag` as if GitHub had redirected.
    fn new(tag: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bin).unwrap();
        // Two sources, so the commands used to provoke the passive notice
        // have something to compare and fail on their own terms.
        for name in ["a.rs", "b.rs"] {
            fs::write(home.join(name), "fn f() -> i32 { 1 }\n").unwrap();
        }

        let script = match tag {
            Some(tag) => format!(
                "#!/bin/sh\nprintf '%s' 'https://github.com/Krv-Labs/topos/releases/tag/{tag}'\n"
            ),
            // Exit non-zero with no output, like a machine with no network.
            None => "#!/bin/sh\nexit 7\n".to_string(),
        };
        let curl = bin.join("curl");
        fs::write(&curl, script).unwrap();
        make_executable(&curl);
        Self {
            _dir: dir,
            home,
            bin,
        }
    }

    /// Run `topos` with no terminal on any stream and `CI` unset, so the only
    /// thing stopping a prompt is the code under test.
    fn run(&self, args: &[&str]) -> Run {
        let out = self.home.parent().unwrap().join("out");
        let err = self.home.parent().unwrap().join("err");
        let mut child = Command::new(env!("CARGO_BIN_EXE_topos"))
            .args(args)
            .current_dir(&self.home)
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .env("NO_COLOR", "1")
            .env_remove("CI")
            .env_remove("XDG_STATE_HOME")
            .env_remove("TOPOS_NO_UPDATE_NOTICES")
            .stdin(Stdio::null())
            .stdout(fs::File::create(&out).unwrap())
            .stderr(fs::File::create(&err).unwrap())
            .spawn()
            .expect("spawn topos");
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
            std::thread::sleep(Duration::from_millis(25));
        };
        Run {
            code: status.code().unwrap_or(-1),
            stdout: fs::read_to_string(&out).unwrap(),
            stderr: fs::read_to_string(&err).unwrap(),
        }
    }

    fn state_dir(&self) -> PathBuf {
        topos_mcp::paths::state_dir(&self.home)
    }
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn assert_no_prompt(run: &Run) {
    for mark in PROMPT_MARKS {
        assert!(!run.stderr.contains(mark), "{mark:?} in:\n{}", run.stderr);
        assert!(!run.stdout.contains(mark), "{mark:?} in:\n{}", run.stdout);
    }
}

/// `topos update --json` reports the survey without touching anything.
#[test]
fn json_reports_the_install_survey() {
    let fixture = Fixture::new(Some("v0.7.1"));
    let run = fixture.run(&["update", "--json"]);
    assert_eq!(run.code, 0, "stderr:\n{}", run.stderr);

    let survey: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{}", run.stdout));
    assert_eq!(survey["latest"], "0.7.1");
    assert_eq!(
        survey["current"],
        env!("CARGO_PKG_VERSION"),
        "the running version comes from Cargo.toml"
    );
    assert_eq!(survey["updateAvailable"], true);
    assert!(
        survey["platform"].as_str().unwrap().contains('-'),
        "a platform label is expected: {survey}"
    );
    // Every install carries the command that upgrades it, so a caller can act
    // on the JSON without knowing the channel rules.
    for install in survey["installs"].as_array().expect("installs array") {
        assert!(install["path"].is_string());
        assert!(install["channel"].is_string());
        assert!(install["command"].is_string());
        assert!(install["updatable"].is_boolean());
    }
}

/// The cache is written where uninstall expects to find it, and its shape is
/// the one the notices read.
///
/// The once-per-24h behavior itself is asserted in the `topos-mcp` unit tests:
/// the e2e suite runs with `Stdio::null()` on every stream, so the terminal
/// check that gates the notice is unreachable from here, and asserting silence
/// would pass whether or not the throttle works.
#[test]
fn an_explicit_run_caches_the_survey_for_the_passive_notice() {
    let fixture = Fixture::new(Some("v99.0.0"));
    fixture.run(&["update", "--json"]);

    let cache = fixture.state_dir().join("update-check.json");
    assert!(
        cache.exists(),
        "an explicit run should cache what it learned"
    );

    let record: Value = serde_json::from_str(&fs::read_to_string(&cache).unwrap())
        .expect("the cache must be valid JSON, so a truncated write is detectable");
    assert_eq!(record["latest"], "99.0.0");
    assert!(record["checked_at"].as_u64().unwrap() > 0);
    // `notified_at` starts unset: an explicit run has not *notified* anyone,
    // and clobbering this would either suppress or duplicate the next notice.
    assert_eq!(record["notified_at"], 0);
}

/// A version that is not newer produces a clean report, not an offer.
#[test]
fn the_same_version_is_not_offered_as_an_update() {
    let fixture = Fixture::new(Some(&format!("v{}", env!("CARGO_PKG_VERSION"))));
    let run = fixture.run(&["update", "--json"]);
    assert_eq!(run.code, 0);
    let survey: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(survey["updateAvailable"], false);
}

/// An unreachable release server is silence, not a failure and not a verdict.
#[test]
fn an_unreachable_release_server_reports_no_latest() {
    let fixture = Fixture::new(None);
    let run = fixture.run(&["update", "--json"]);
    assert_eq!(run.code, 0, "a network failure must not fail the command");
    let survey: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(survey["latest"], Value::Null);
    assert_eq!(
        survey["updateAvailable"],
        Value::Null,
        "None is not the same as false"
    );
}

/// The load-bearing safety property: no terminal means no prompt and no change.
#[test]
fn a_non_interactive_run_never_prompts() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let run = fixture.run(&["update"]);
    assert_eq!(run.code, 0, "stderr:\n{}", run.stderr);
    assert_no_prompt(&run);
    // It must say what to run instead of waiting.
    assert!(
        run.stdout.contains("not a terminal"),
        "expected the manual instruction:\n{}",
        run.stdout
    );
}

/// `--check` is strictly read-only, even on a terminal-shaped run.
#[test]
fn check_reports_without_offering() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let run = fixture.run(&["update", "--check"]);
    assert_eq!(run.code, 0);
    assert_no_prompt(&run);
    assert!(
        !run.stdout.contains("Continue with current version"),
        "--check must not open the prompt:\n{}",
        run.stdout
    );
}

/// The passive notice writes a cache but never prompts, and the opt-out
/// silences it entirely.
/// The opt-out silences the *passive* notice without breaking an explicit run.
///
/// A redirected run is silent regardless, because the notice requires a
/// terminal — so what this pins is that the env var leaves `topos update`
/// working, which is the distinction a user hits when they want the command
/// but not the nagging.
#[test]
fn the_opt_out_is_respected() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(["update", "--json"])
        .current_dir(&fixture.home)
        .env("HOME", &fixture.home)
        .env("PATH", &fixture.bin)
        .env("NO_COLOR", "1")
        .env_remove("CI")
        .env("TOPOS_NO_UPDATE_NOTICES", "1")
        .stdin(Stdio::null())
        .output()
        .expect("run topos update");
    assert_eq!(output.status.code(), Some(0));
    let survey: Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert_eq!(
        survey["latest"], "99.0.0",
        "an explicit request still answers; the opt-out silences passive notices"
    );
}

/// CI gets no unprompted network traffic at all — no cache file written.
#[test]
fn ci_writes_no_cache() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let run = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(["compare", "a.rs", "b.rs"])
        .current_dir(&fixture.home)
        .env("HOME", &fixture.home)
        .env("PATH", &fixture.bin)
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .stdin(Stdio::null())
        .output()
        .expect("run compare");
    assert!(
        !fixture.state_dir().join("update-check.json").exists(),
        "CI must not be written a cache it will never read"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        !stderr.contains("topos update"),
        "CI must stay quiet:\n{stderr}"
    );
}

#[test]
fn uninstall_removes_the_update_cache() {
    let fixture = Fixture::new(Some("v99.0.0"));
    fixture.run(&["update", "--json"]);
    let cache = fixture.state_dir().join("update-check.json");
    assert!(cache.exists());

    // Install first: the cache is only taken down as part of a *full*
    // teardown, because an uninstall with nothing to do must leave the tree
    // exactly as it found it (asserted in `install_e2e`).
    let install = fixture.run(&["install", "claude", "--dry-run"]);
    assert_eq!(install.code, 0, "stderr:\n{}", install.stderr);
    let applied = fixture.run(&["install", "claude"]);
    assert_eq!(applied.code, 0, "stderr:\n{}", applied.stderr);

    let run = fixture.run(&["uninstall", "--all"]);
    assert_eq!(run.code, 0, "stderr:\n{}", run.stderr);
    assert!(
        !cache.exists(),
        "uninstall left the update cache behind at {}",
        cache.display()
    );
}

/// `topos update` is listed in root help, since it is a real command.
#[test]
fn update_appears_in_root_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .arg("--help")
        .output()
        .expect("run topos --help");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("update"), "missing from root help:\n{help}");
    assert!(
        help.contains("Check for and install a newer release"),
        "missing description:\n{help}"
    );
}

/// The long help explains the channel rules, which are the part a person
/// needs before letting the command near their machine.
#[test]
fn the_long_help_documents_every_channel() {
    let output = Command::new(env!("CARGO_BIN_EXE_topos"))
        .args(["update", "--help"])
        .output()
        .expect("run topos update --help");
    let help = String::from_utf8_lossy(&output.stdout);
    for channel in ["binary install", "homebrew", "cargo", "source checkout"] {
        assert!(
            help.contains(channel),
            "missing {channel} from help:\n{help}"
        );
    }
    assert!(
        help.contains("PATH"),
        "the multi-install caveat must be documented:\n{help}"
    );
    assert!(
        help.contains("TOPOS_NO_UPDATE_NOTICES"),
        "the opt-out must be documented:\n{help}"
    );
}

/// Fake release assets exercise the checksum and replacement path without network access.
#[cfg(unix)]
fn binary_release(fixture: &Fixture, corrupted: bool) -> (PathBuf, Vec<u8>) {
    let dir = fixture.home.join(".local/bin");
    fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("topos");
    fs::write(&binary, "#!/bin/sh\necho 'topos 0.1.0'\n").unwrap();
    make_executable(&binary);
    let payload = b"#!/bin/sh\necho 'topos 99.0.0'\n".to_vec();
    let asset = fixture.home.join("release-asset");
    fs::write(&asset, &payload).unwrap();
    let digest = topos_mcp::update::checksums::sha256_file(&asset).unwrap();
    if corrupted {
        fs::write(&asset, "corrupt transfer").unwrap();
    }
    // Paths come from tempfile, not shell input.
    let script = format!(
        "#!/bin/sh\nhead=0\nfor arg do\n  [ \"$arg\" = --head ] && head=1\n  last=$arg\ndone\nif [ \"$head\" = 1 ]; then\n  printf 'Content-Length: 30\r\n'\n  exit 0\nfi\ncase \"$last\" in\n  */releases/latest) printf '%s' 'https://github.com/Krv-Labs/topos/releases/tag/v99.0.0' ;;\n  */checksums.txt) printf '%s\n' '{digest}  topos-{}' ;;\n  *) /bin/cat '{}' ;;\nesac\n",
        topos_mcp::update::release::platform(), asset.display()
    );
    fs::write(fixture.bin.join("curl"), script).unwrap();
    (binary, payload)
}

#[cfg(unix)]
#[test]
fn yes_installs_a_verified_binary_without_prompting() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let (binary, payload) = binary_release(&fixture, false);
    let run = fixture.run(&["update", "--yes"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_no_prompt(&run);
    assert_eq!(fs::read(&binary).unwrap(), payload);
    assert!(run.stdout.contains("Topos updated"));
}

#[cfg(unix)]
#[test]
fn a_bad_checksum_preserves_the_binary_and_removes_staging() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let (binary, _) = binary_release(&fixture, true);
    let original = fs::read(&binary).unwrap();
    let run = fixture.run(&["update", "--yes"]);
    assert_eq!(run.code, 1, "stderr: {}", run.stderr);
    assert!(run.stderr.contains("checksum mismatch"), "{}", run.stderr);
    assert_eq!(fs::read(&binary).unwrap(), original);
    let entries: Vec<_> = fs::read_dir(binary.parent().unwrap()).unwrap().collect();
    assert_eq!(entries.len(), 1, "staging files must be cleaned on failure");
}

#[test]
fn redirected_commands_do_not_fetch_or_write_passive_state() {
    let fixture = Fixture::new(Some("v99.0.0"));
    let run = fixture.run(&["compare", "a.rs", "b.rs"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(!fixture.state_dir().join("update-check.json").exists());
    assert!(!run.stderr.contains("topos update"));
}
