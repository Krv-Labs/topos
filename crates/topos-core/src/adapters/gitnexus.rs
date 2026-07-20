//! Shared GitNexus dependency-graph generation.
//!
//! A single place for the `gitnexus analyze` invocation, so the CLI
//! (`topos depgraph generate`) and the MCP tool (`topos_generate_depgraph`)
//! stay in lockstep. This is the *generation* side — shelling out to run
//! [GitNexus](https://github.com/abhigyanpatwari/GitNexus) itself and
//! fingerprinting the source tree it ran against. The *consumer* side —
//! reading an already-generated `.gitnexus/` store back into a
//! [`crate::graphs::mdg::ModuleDependencyGraph`] — is a different concern,
//! already ported at [`crate::graphs::mdg::object::ModuleDependencyGraph::from_gitnexus_dir`].
//!
//! # Deviation from the Python original
//! - `subprocess.run(..., timeout=...)` has no direct `std::process`
//!   equivalent; the actual process spawn/wait/kill lives in
//!   [`super::process::run_with_timeout`] — see that module's doc comment.
//! - `shutil.which` is replaced by a small manual `$PATH` scan
//!   ([`command_on_path`]) rather than a `which` crate dependency: the
//!   full semantics (Windows `PATHEXT`, symlink resolution, ...) aren't
//!   needed for finding one specific binary.
//! - The source-fingerprint hash uses BLAKE2b (via the `blake2` crate,
//!   already a workspace dependency for `UASTNode::id` — see
//!   `crate::graphs::uast::mapper_common`) rather than Python's SHA-256.
//!   The digest algorithm is an implementation detail here: the fingerprint
//!   is written and later re-derived by the *same* tool for its own
//!   staleness check, never compared byte-for-byte against a value some
//!   other program computed, so there's no cross-language parity
//!   requirement (unlike `UASTNode::id`, which Python and Rust may both
//!   compute for the same source and must agree on).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

use super::discovery::iter_source_files;
use super::process::{run_with_timeout, RunError};
use crate::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

const GITNEXUS_CMD: &str = "gitnexus";
const INSTALL_HINT: &str = "GitNexus not found. Install it with: npm install -g gitnexus";

/// Topos-owned marker written inside `.gitnexus` recording what source
/// snapshot the graph was built from. v1 markers carried only `head_sha`;
/// v2 added `generated_at`/`finished_at`; current markers add a source
/// content hash so freshness is not decided by fragile filesystem clocks.
pub const GITNEXUS_FINGERPRINT_FILE: &str = ".topos-fingerprint.json";

/// `gitnexus analyze` can legitimately run for minutes on a large repo, so
/// the default ceiling is deliberately generous. Operators can override it
/// via the `TOPOS_DEPGRAPH_TIMEOUT` env var (seconds); set it to 0 to
/// disable entirely.
pub const DEFAULT_ANALYZE_TIMEOUT_S: f64 = 300.0;

const TIMEOUT_RC: i32 = 124; // conventional "timed out" exit code
const EXEC_ERROR_RC: i32 = 126; // command found but could not be executed
const NOT_FOUND_RC: i32 = 127;

/// Resolve the effective subprocess timeout in seconds.
///
/// `None` (the default) falls back to `TOPOS_DEPGRAPH_TIMEOUT` or
/// [`DEFAULT_ANALYZE_TIMEOUT_S`]. A non-positive value disables the timeout.
fn resolve_timeout(timeout: Option<f64>) -> Option<f64> {
    if let Some(t) = timeout {
        return if t > 0.0 { Some(t) } else { None };
    }
    let parsed = std::env::var("TOPOS_DEPGRAPH_TIMEOUT")
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .unwrap_or(DEFAULT_ANALYZE_TIMEOUT_S);
    if parsed > 0.0 {
        Some(parsed)
    } else {
        None
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Whether `name` resolves to an executable file on `$PATH` (a small
/// stand-in for `shutil.which`; see the module "Deviation" note).
fn command_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| is_executable_file(&dir.join(name)))
}

/// Whether the `gitnexus` CLI is on `$PATH`.
pub fn gitnexus_available() -> bool {
    command_on_path(GITNEXUS_CMD)
}

/// Outcome of a `gitnexus analyze` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepgraphGenerationResult {
    pub ok: bool,
    pub returncode: i32,
    pub gitnexus_path: Option<PathBuf>,
    pub message: String,
}

/// Stable content identity for source files seen by GitNexus/Topos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub content_hash: String,
    pub file_count: usize,
}

/// Hash source-file paths and bytes under `root` using existing discovery.
pub fn source_fingerprint(root: &Path) -> SourceFingerprint {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let suffixes: Vec<&str> = {
        let mut all: Vec<&str> = SUPPORTED_LANGUAGES
            .iter()
            .filter_map(|lang| language_file_suffixes(lang))
            .flat_map(|group| group.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    };

    let mut files = iter_source_files(&root, &suffixes, true, None, false);
    files.sort_by(|a, b| {
        a.strip_prefix(&root)
            .unwrap_or(a)
            .cmp(b.strip_prefix(&root).unwrap_or(b))
    });

    let mut hasher = Blake2bVar::new(32).expect("32 is a valid BLAKE2b-var digest size");
    let mut count = 0usize;
    for path in &files {
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        count += 1;
        hasher.update(rel.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update(b"\0");
        hasher.update(metadata.len().to_string().as_bytes());
        hasher.update(b"\0");
        if let Ok(bytes) = std::fs::read(path) {
            hasher.update(&bytes);
        }
        hasher.update(b"\0");
    }
    let mut digest = [0u8; 32];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest buffer matches the requested output size");
    let content_hash = digest.iter().map(|b| format!("{b:02x}")).collect();

    SourceFingerprint {
        content_hash,
        file_count: count,
    }
}

/// Current HEAD commit SHA, or `None` when there is no resolvable commit.
///
/// A missing `.git`, an unborn HEAD (no commits yet), or a detached HEAD
/// with no ref are all normal for the directories GitNexus can analyze —
/// treat them as "no fingerprint" rather than an error.
fn head_sha(target_dir: &Path) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C"]).arg(target_dir).args(["rev-parse", "HEAD"]);
    let output = run_with_timeout(cmd, None, true, Some(Duration::from_secs(5))).ok()?;
    if output.status_code != Some(0) {
        return None;
    }
    let sha = output.stdout.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// Record what the graph was built from (best-effort).
///
/// Never panics or surfaces an error: a write failure must not turn a
/// successful generation into a failure.
fn write_fingerprint(
    target_dir: &Path,
    gitnexus_path: &Path,
    start_time: SystemTime,
    source_snapshot: &SourceFingerprint,
) {
    let sha = head_sha(target_dir);
    let now = SystemTime::now();
    let payload = serde_json::json!({
        "head_sha": sha,
        "generated_at": start_time.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
        "finished_at": now.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
        "source_hash": source_snapshot.content_hash,
        "source_file_count": source_snapshot.file_count,
    });
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = std::fs::write(gitnexus_path.join(GITNEXUS_FINGERPRINT_FILE), text);
    }
}

/// Run `gitnexus analyze --skip-agents-md` in `target_dir`.
///
/// `capture = false` streams gitnexus output to the inherited stdio (used
/// by the CLI); `capture = true` collects it into `message` (used by MCP).
///
/// `timeout` bounds the subprocess in seconds; `None` uses
/// `TOPOS_DEPGRAPH_TIMEOUT` or [`DEFAULT_ANALYZE_TIMEOUT_S`]. A hung or
/// unrunnable `gitnexus` is converted into a structured failure result
/// rather than blocking the caller or panicking, so callers always get a
/// deterministic result.
pub fn generate_depgraph(
    target_dir: &Path,
    capture: bool,
    timeout: Option<f64>,
) -> DepgraphGenerationResult {
    if !gitnexus_available() {
        return not_found_result();
    }
    run_analyze(
        target_dir,
        GITNEXUS_CMD,
        &["analyze", "--skip-agents-md"],
        capture,
        timeout,
    )
}

/// The "`gitnexus` isn't on `$PATH`" result — pulled out so tests can
/// assert on it directly rather than depend on the test machine actually
/// lacking `gitnexus` (this repo's own dev boxes may well have it
/// installed for dogfooding).
fn not_found_result() -> DepgraphGenerationResult {
    DepgraphGenerationResult {
        ok: false,
        returncode: NOT_FOUND_RC,
        gitnexus_path: None,
        message: INSTALL_HINT.to_string(),
    }
}

/// The part of [`generate_depgraph`] after the availability check —
/// pulled out so tests can point it at a stand-in command (and args)
/// instead of the real `gitnexus` binary (which the test environment
/// won't have installed).
fn run_analyze(
    target_dir: &Path,
    cmd_name: &str,
    args: &[&str],
    capture: bool,
    timeout: Option<f64>,
) -> DepgraphGenerationResult {
    let start_time = SystemTime::now();
    let source_snapshot = source_fingerprint(target_dir);
    let effective_timeout = resolve_timeout(timeout);
    let duration_timeout = effective_timeout.map(Duration::from_secs_f64);

    let mut cmd = Command::new(cmd_name);
    cmd.args(args);

    match run_with_timeout(cmd, Some(target_dir), capture, duration_timeout) {
        Err(RunError::TimedOut) => timed_out_result(effective_timeout),
        Err(RunError::Io(exc)) => io_error_result(&exc),
        Ok(output) if output.status_code.unwrap_or(-1) != 0 => failed_result(&output, capture),
        Ok(output) => finished_result(target_dir, &output, capture, start_time, &source_snapshot),
    }
}

fn timed_out_result(effective_timeout: Option<f64>) -> DepgraphGenerationResult {
    let limit = effective_timeout
        .map(|t| format!("{t:.0}s"))
        .unwrap_or_else(|| "the limit".to_string());
    DepgraphGenerationResult {
        ok: false,
        returncode: TIMEOUT_RC,
        gitnexus_path: None,
        message: format!(
            "gitnexus analyze timed out after {limit}; raise TOPOS_DEPGRAPH_TIMEOUT or run it manually."
        ),
    }
}

fn io_error_result(exc: &std::io::Error) -> DepgraphGenerationResult {
    DepgraphGenerationResult {
        ok: false,
        returncode: EXEC_ERROR_RC,
        gitnexus_path: None,
        message: format!("gitnexus analyze could not be executed: {exc}"),
    }
}

fn failed_result(output: &super::process::RunOutput, capture: bool) -> DepgraphGenerationResult {
    let stderr = output.stderr.trim();
    let stdout = output.stdout.trim();
    let detail = if !capture {
        ""
    } else if !stderr.is_empty() {
        stderr
    } else {
        stdout
    };
    DepgraphGenerationResult {
        ok: false,
        returncode: output.status_code.unwrap_or(-1),
        gitnexus_path: None,
        message: if detail.is_empty() {
            "gitnexus analyze failed.".to_string()
        } else {
            detail.to_string()
        },
    }
}

fn finished_result(
    target_dir: &Path,
    output: &super::process::RunOutput,
    capture: bool,
    start_time: SystemTime,
    source_snapshot: &SourceFingerprint,
) -> DepgraphGenerationResult {
    let gitnexus_path = target_dir.join(".gitnexus");
    write_fingerprint(target_dir, &gitnexus_path, start_time, source_snapshot);
    let detail = if capture { output.stdout.trim() } else { "" };
    DepgraphGenerationResult {
        ok: true,
        returncode: 0,
        gitnexus_path: Some(gitnexus_path.clone()),
        message: if detail.is_empty() {
            format!("Dependency graph written to {}", gitnexus_path.display())
        } else {
            detail.to_string()
        },
    }
}

/// GitNexus's per-store metadata file (written next to each `lbug` store).
pub const GITNEXUS_META_FILE: &str = "meta.json";
/// Directory of branch-scoped stores under `.gitnexus/`.
pub const GITNEXUS_BRANCHES_DIR: &str = "branches";

/// Parsed `meta.json` for one `lbug` store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LbugStoreMeta {
    pub branch: String,
    pub last_commit: Option<String>,
}

/// The `lbug` store to load for a given branch, or a "no match" report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedLbugStore {
    pub path: Option<PathBuf>,
    pub matched_branch: Option<String>,
    pub available_branches: Vec<String>,
}

/// Best-effort parse of GitNexus's `meta.json` next to a store. Returns
/// `None` on any read/parse error or a missing `"branch"` key — callers
/// treat that identically to "no metadata, can't branch-match."
fn read_store_meta(store_dir: &Path) -> Option<LbugStoreMeta> {
    let raw = std::fs::read_to_string(store_dir.join(GITNEXUS_META_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let branch = value.get("branch")?.as_str()?.to_string();
    let last_commit = value
        .get("lastCommit")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Some(LbugStoreMeta {
        branch,
        last_commit,
    })
}

/// Current branch name, parsed from `.git/HEAD` without shelling out.
///
/// Returns `None` for a non-git dir, a detached HEAD (HEAD holds a raw
/// SHA, not a `ref:` line), or a ref outside `refs/heads/` (e.g. a tag
/// checkout) — all of which mean "no branch identity to match against,"
/// and callers fall back to the flat store unconditionally.
pub fn current_git_branch(project_root: &Path) -> Option<String> {
    let head_text = std::fs::read_to_string(project_root.join(".git").join("HEAD")).ok()?;
    let head_text = head_text.trim();
    let ref_path = head_text.strip_prefix("ref: ")?.trim();
    let branch = ref_path.strip_prefix("refs/heads/")?;
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

/// Pick the `lbug` store under `gitnexus_dir` matching `current_branch`.
///
/// 1. `current_branch` is `None` (no git / detached HEAD), or the flat
///    store exists with no `meta.json` (a legacy, pre-branch-aware store)
///    → the flat slot unconditionally.
/// 2. The flat store's `meta.json` records `current_branch` → flat slot.
/// 3. Otherwise scan `branches/*/meta.json` for one recording
///    `current_branch` (matched on the `meta.json` content, never on the
///    opaque hash suffix GitNexus appends to the directory name).
/// 4. No match anywhere → `path: None`, `available_branches` populated
///    (sorted) so the caller can build an actionable error message.
pub fn resolve_lbug_store(gitnexus_dir: &Path, current_branch: Option<&str>) -> ResolvedLbugStore {
    let flat_lbug = gitnexus_dir.join("lbug");
    let flat_meta = read_store_meta(gitnexus_dir);

    let Some(current_branch) = current_branch else {
        return ResolvedLbugStore {
            path: flat_lbug.exists().then_some(flat_lbug),
            matched_branch: flat_meta.map(|m| m.branch),
            available_branches: Vec::new(),
        };
    };
    if flat_lbug.exists() && flat_meta.is_none() {
        return ResolvedLbugStore {
            path: Some(flat_lbug),
            matched_branch: None,
            available_branches: Vec::new(),
        };
    }

    if let Some(meta) = &flat_meta {
        if meta.branch == current_branch {
            return ResolvedLbugStore {
                path: Some(flat_lbug),
                matched_branch: Some(current_branch.to_string()),
                available_branches: Vec::new(),
            };
        }
    }

    let mut available: Vec<String> = flat_meta.iter().map(|m| m.branch.clone()).collect();
    let branches_dir = gitnexus_dir.join(GITNEXUS_BRANCHES_DIR);
    if branches_dir.is_dir() {
        let mut candidates: Vec<PathBuf> = std::fs::read_dir(&branches_dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        candidates.sort();
        for candidate in candidates {
            let Some(meta) = read_store_meta(&candidate) else {
                continue;
            };
            available.push(meta.branch.clone());
            if meta.branch == current_branch {
                let lbug = candidate.join("lbug");
                if lbug.exists() {
                    return ResolvedLbugStore {
                        path: Some(lbug),
                        matched_branch: Some(current_branch.to_string()),
                        available_branches: Vec::new(),
                    };
                }
            }
        }
    }

    available.sort();
    ResolvedLbugStore {
        path: None,
        matched_branch: None,
        available_branches: available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_gitnexus_test_{label}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_repo(root: &Path) -> String {
        let run = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
        };
        run(&["init"]);
        run(&["config", "user.email", "t@t.t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(root.join("f.py"), "x = 1\n").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-m", "init"]);
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn missing_gitnexus_returns_structured_failure() {
        // Doesn't depend on the environment actually lacking `gitnexus`
        // (see `not_found_result`'s doc comment) — asserts on the exact
        // result `generate_depgraph` returns when the check fails.
        let result = not_found_result();
        assert!(!result.ok);
        assert_eq!(result.returncode, 127);
        assert!(result.message.contains("npm install -g gitnexus"));
    }

    #[test]
    fn timeout_is_converted_to_structured_failure() {
        let tmp = unique_tmp_dir("timeout");
        let result = run_analyze(&tmp, "sleep", &["5"], true, Some(0.1));
        assert!(!result.ok);
        assert_eq!(result.returncode, 124);
        assert!(result.message.contains("timed out"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn nonexistent_command_is_converted_to_structured_failure() {
        let tmp = unique_tmp_dir("oserror");
        let result = run_analyze(&tmp, "topos-nonexistent-gitnexus-xyz", &[], true, None);
        assert!(!result.ok);
        assert_eq!(result.returncode, 126);
        assert!(result.message.contains("could not be executed"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn env_var_overrides_and_disables_timeout() {
        std::env::set_var("TOPOS_DEPGRAPH_TIMEOUT", "42");
        assert_eq!(resolve_timeout(None), Some(42.0));
        std::env::set_var("TOPOS_DEPGRAPH_TIMEOUT", "0"); // non-positive disables
        assert_eq!(resolve_timeout(None), None);
        std::env::set_var("TOPOS_DEPGRAPH_TIMEOUT", "garbage"); // falls back
        assert_eq!(resolve_timeout(None), Some(DEFAULT_ANALYZE_TIMEOUT_S));
        // An explicit argument wins over the env var.
        assert_eq!(resolve_timeout(Some(10.0)), Some(10.0));
        std::env::remove_var("TOPOS_DEPGRAPH_TIMEOUT");
    }

    #[test]
    fn generate_writes_fingerprint_with_head_sha() {
        let tmp = unique_tmp_dir("fingerprint_head_sha");
        let head = init_repo(&tmp);
        std::fs::create_dir_all(tmp.join(".gitnexus")).unwrap();

        // Stand in for `gitnexus analyze` with `true`, a real binary that
        // exits 0 with no output — exercises the full success path
        // (fingerprint + write) without needing gitnexus installed.
        let result = run_analyze(&tmp, "true", &[], true, None);
        assert!(result.ok);

        let marker = tmp.join(".gitnexus").join(GITNEXUS_FINGERPRINT_FILE);
        assert!(marker.exists());
        let payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&marker).unwrap()).unwrap();
        assert_eq!(payload["head_sha"], serde_json::json!(head));
        assert!(payload["generated_at"].as_f64().unwrap() > 0.0);
        assert_eq!(
            payload["source_hash"],
            serde_json::json!(source_fingerprint(&tmp).content_hash)
        );
        assert_eq!(payload["source_file_count"], serde_json::json!(1));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn generate_in_non_git_dir_writes_sha_less_fingerprint() {
        let tmp = unique_tmp_dir("fingerprint_no_git");
        std::fs::create_dir_all(tmp.join(".gitnexus")).unwrap();

        let result = run_analyze(&tmp, "true", &[], true, None);
        assert!(result.ok);

        let marker = tmp.join(".gitnexus").join(GITNEXUS_FINGERPRINT_FILE);
        let payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&marker).unwrap()).unwrap();
        assert_eq!(payload["head_sha"], serde_json::Value::Null);
        assert!(payload["generated_at"].as_f64().unwrap() > 0.0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn failing_command_reports_captured_output() {
        let tmp = unique_tmp_dir("failure");
        let result = run_analyze(&tmp, "false", &[], true, None);
        assert!(!result.ok);
        assert_eq!(result.returncode, 1);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
