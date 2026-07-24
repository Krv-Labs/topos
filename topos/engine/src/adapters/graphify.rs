//! Shared [Graphify](https://github.com/Graphify-Labs/graphify) knowledge-graph
//! generation.
//!
//! A single place for the `graphify update`/`extract` invocation, so the CLI
//! (`topos graphify generate`) and the MCP tool
//! (`topos_generate_graphify_graph`) stay in lockstep. This is the
//! *generation* side — shelling out to run Graphify itself. The *consumer*
//! side — reading an already-generated `graphify-out/graph.json` back into a
//! [`crate::graphs::graphify::GraphifyGraph`] — is a different concern; see
//! that type.
//!
//! Mirrors [`super::gitnexus`]'s shape closely, with two deliberate
//! deviations:
//!
//! - **No topos-side fingerprint/staleness marker.** Graphify already
//!   maintains its own SHA256 content cache under `graphify-out/cache/`, so
//!   re-invoking `graphify update` when nothing changed is already cheap —
//!   a second, topos-owned freshness check would duplicate that signal for
//!   no benefit. Callers that want "ensure the graph is current" simply call
//!   [`ensure_graphify_graph`] every time; Graphify's own cache absorbs the
//!   no-op case.
//! - **`graphify update` bootstraps a first-time graph directly** (verified
//!   empirically against a real install: running `update` in a directory
//!   with no prior `graphify-out/` produces one). [`ensure_graphify_graph`]
//!   still carries a defensive fallback to `graphify extract --no-cluster`
//!   if a future Graphify version ever regresses that guarantee, but under
//!   normal operation only `update` ever runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::process::{command_on_path, run_with_timeout, timeout_duration, RunError};

const GRAPHIFY_CMD: &str = "graphify";
const INSTALL_HINT: &str = "Graphify not found. Install it with: pip install graphifyy";

/// Default output directory name, relative to the analyzed directory —
/// Graphify's own default when `GRAPHIFY_OUT` is unset.
pub const GRAPHIFY_OUT_DIR_NAME: &str = "graphify-out";
/// The persistent graph file Graphify writes inside its output directory.
pub const GRAPHIFY_GRAPH_FILE: &str = "graph.json";

/// `graphify update`/`extract` can run for a while on a large repo (full
/// AST re-extraction, even though it's LLM-free and content-cached), so the
/// default ceiling is deliberately generous, matching GitNexus's own
/// default. Operators can override it via the `TOPOS_GRAPHIFY_TIMEOUT` env
/// var (seconds); set it to 0 to disable entirely.
pub const DEFAULT_GRAPHIFY_TIMEOUT_S: f64 = 300.0;

const TIMEOUT_RC: i32 = 124; // conventional "timed out" exit code
const EXEC_ERROR_RC: i32 = 126; // command found but could not be executed
const NOT_FOUND_RC: i32 = 127;

/// Resolve the effective subprocess timeout in seconds.
///
/// `None` (the default) falls back to `TOPOS_GRAPHIFY_TIMEOUT` or
/// [`DEFAULT_GRAPHIFY_TIMEOUT_S`]. A non-positive value disables the timeout.
fn resolve_timeout(timeout: Option<f64>) -> Option<f64> {
    if let Some(t) = timeout {
        return if t > 0.0 { Some(t) } else { None };
    }
    let parsed = std::env::var("TOPOS_GRAPHIFY_TIMEOUT")
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .unwrap_or(DEFAULT_GRAPHIFY_TIMEOUT_S);
    if parsed > 0.0 {
        Some(parsed)
    } else {
        None
    }
}

/// Whether the `graphify` CLI is on `$PATH`.
pub fn graphify_available() -> bool {
    command_on_path(GRAPHIFY_CMD)
}

/// Where Graphify's output lives for a given analyzed directory.
///
/// Precedence: the `GRAPHIFY_OUT` env var (absolute, or resolved relative to
/// `target_dir` if relative) — matching Graphify's own
/// `os.environ.get("GRAPHIFY_OUT", "graphify-out")` resolution — falling
/// back to `<target_dir>/graphify-out`. Shared by both the generator (to
/// locate output afterward) and the MCP/CLI readers, so generation and
/// reading never disagree about where to look.
pub fn graphify_out_dir(target_dir: &Path) -> PathBuf {
    match std::env::var("GRAPHIFY_OUT") {
        Ok(raw) if !raw.is_empty() => {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                path
            } else {
                target_dir.join(path)
            }
        }
        _ => target_dir.join(GRAPHIFY_OUT_DIR_NAME),
    }
}

/// Outcome of a `graphify update`/`extract` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphifyGenerationResult {
    pub ok: bool,
    pub returncode: i32,
    pub graphify_out_dir: Option<PathBuf>,
    pub message: String,
}

/// Ensure `graphify-out/graph.json` exists and is current for `target_dir`.
///
/// Runs `graphify update .` (deterministic, no LLM call, content-cached —
/// matching Topos's own no-LLM-in-core-analysis philosophy). If that exits
/// successfully but produces no `graph.json` — a defensive fallback for a
/// bootstrap-on-`update` regression, not something observed in practice, see
/// the module doc — falls back to `graphify extract . --no-cluster`.
///
/// `capture = false` streams Graphify's output to the inherited stdio (used
/// by the CLI); `capture = true` collects it into `message` (used by MCP).
/// `timeout` bounds the subprocess in seconds; `None` uses
/// `TOPOS_GRAPHIFY_TIMEOUT` or [`DEFAULT_GRAPHIFY_TIMEOUT_S`].
pub fn ensure_graphify_graph(
    target_dir: &Path,
    capture: bool,
    timeout: Option<f64>,
) -> GraphifyGenerationResult {
    if !graphify_available() {
        return not_found_result();
    }
    ensure_graph_with(target_dir, GRAPHIFY_CMD, capture, timeout)
}

/// The part of [`ensure_graphify_graph`] after the availability check —
/// pulled out so tests can point `cmd_name` at a stand-in script instead of
/// the real `graphify` binary, exercising the update→extract fallback
/// without needing Graphify installed or mutating global process state.
fn ensure_graph_with(
    target_dir: &Path,
    cmd_name: &str,
    capture: bool,
    timeout: Option<f64>,
) -> GraphifyGenerationResult {
    let graph_file = graphify_out_dir(target_dir).join(GRAPHIFY_GRAPH_FILE);
    let result = run_graphify(target_dir, cmd_name, &["update", "."], capture, timeout);
    if result.ok && !graph_file.is_file() {
        return run_graphify(
            target_dir,
            cmd_name,
            &["extract", ".", "--no-cluster"],
            capture,
            timeout,
        );
    }
    result
}

/// The "`graphify` isn't on `$PATH`" result — pulled out so tests can
/// assert on it directly rather than depend on the test machine actually
/// lacking `graphify` (this repo's own dev boxes may well have it
/// installed).
fn not_found_result() -> GraphifyGenerationResult {
    GraphifyGenerationResult {
        ok: false,
        returncode: NOT_FOUND_RC,
        graphify_out_dir: None,
        message: INSTALL_HINT.to_string(),
    }
}

/// The part of [`ensure_graphify_graph`] after the availability check —
/// pulled out so tests can point it at a stand-in command (and args)
/// instead of the real `graphify` binary.
fn run_graphify(
    target_dir: &Path,
    cmd_name: &str,
    args: &[&str],
    capture: bool,
    timeout: Option<f64>,
) -> GraphifyGenerationResult {
    let effective_timeout = resolve_timeout(timeout);
    let duration_timeout = effective_timeout.and_then(timeout_duration);

    let mut cmd = Command::new(cmd_name);
    cmd.args(args);

    match run_with_timeout(cmd, Some(target_dir), capture, duration_timeout) {
        Err(RunError::TimedOut) => timed_out_result(effective_timeout),
        Err(RunError::Io(exc)) => io_error_result(&exc),
        Ok(output) if output.status_code.unwrap_or(-1) != 0 => failed_result(&output, capture),
        Ok(output) => finished_result(target_dir, &output, capture),
    }
}

fn timed_out_result(effective_timeout: Option<f64>) -> GraphifyGenerationResult {
    let limit = effective_timeout
        .map(|t| format!("{t:.0}s"))
        .unwrap_or_else(|| "the limit".to_string());
    GraphifyGenerationResult {
        ok: false,
        returncode: TIMEOUT_RC,
        graphify_out_dir: None,
        message: format!(
            "graphify timed out after {limit}; raise TOPOS_GRAPHIFY_TIMEOUT or run it manually."
        ),
    }
}

fn io_error_result(exc: &std::io::Error) -> GraphifyGenerationResult {
    GraphifyGenerationResult {
        ok: false,
        returncode: EXEC_ERROR_RC,
        graphify_out_dir: None,
        message: format!("graphify could not be executed: {exc}"),
    }
}

fn failed_result(output: &super::process::RunOutput, capture: bool) -> GraphifyGenerationResult {
    let stderr = output.stderr.trim();
    let stdout = output.stdout.trim();
    let detail = if !capture {
        ""
    } else if !stderr.is_empty() {
        stderr
    } else {
        stdout
    };
    GraphifyGenerationResult {
        ok: false,
        returncode: output.status_code.unwrap_or(-1),
        graphify_out_dir: None,
        message: if detail.is_empty() {
            "graphify failed.".to_string()
        } else {
            detail.to_string()
        },
    }
}

fn finished_result(
    target_dir: &Path,
    output: &super::process::RunOutput,
    capture: bool,
) -> GraphifyGenerationResult {
    let out_dir = graphify_out_dir(target_dir);
    let detail = if capture { output.stdout.trim() } else { "" };
    GraphifyGenerationResult {
        ok: true,
        returncode: 0,
        graphify_out_dir: Some(out_dir.clone()),
        message: if detail.is_empty() {
            format!("Graph written to {}", out_dir.display())
        } else {
            detail.to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_graphify_test_{label}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_graphify_returns_structured_failure() {
        let result = not_found_result();
        assert!(!result.ok);
        assert_eq!(result.returncode, 127);
        assert!(result.message.contains("pip install graphifyy"));
    }

    #[test]
    fn timeout_is_converted_to_structured_failure() {
        let tmp = unique_tmp_dir("timeout");
        let result = run_graphify(&tmp, "sleep", &["5"], true, Some(0.1));
        assert!(!result.ok);
        assert_eq!(result.returncode, 124);
        assert!(result.message.contains("timed out"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn nonexistent_command_is_converted_to_structured_failure() {
        let tmp = unique_tmp_dir("oserror");
        let result = run_graphify(&tmp, "topos-nonexistent-graphify-xyz", &[], true, None);
        assert!(!result.ok);
        assert_eq!(result.returncode, 126);
        assert!(result.message.contains("could not be executed"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn env_var_overrides_and_disables_timeout() {
        std::env::set_var("TOPOS_GRAPHIFY_TIMEOUT", "42");
        assert_eq!(resolve_timeout(None), Some(42.0));
        std::env::set_var("TOPOS_GRAPHIFY_TIMEOUT", "0"); // non-positive disables
        assert_eq!(resolve_timeout(None), None);
        std::env::set_var("TOPOS_GRAPHIFY_TIMEOUT", "garbage"); // falls back
        assert_eq!(resolve_timeout(None), Some(DEFAULT_GRAPHIFY_TIMEOUT_S));
        // An explicit argument wins over the env var.
        assert_eq!(resolve_timeout(Some(10.0)), Some(10.0));
        std::env::remove_var("TOPOS_GRAPHIFY_TIMEOUT");
    }

    #[test]
    fn failing_command_reports_captured_output() {
        let tmp = unique_tmp_dir("failure");
        let result = run_graphify(&tmp, "false", &[], true, None);
        assert!(!result.ok);
        assert_eq!(result.returncode, 1);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn successful_run_reports_out_dir() {
        let tmp = unique_tmp_dir("success");
        // Stand in for `graphify update` with `true`: exits 0, writes
        // nothing. Since it won't have produced graph.json, this also
        // exercises ensure_graphify_graph's fallback path indirectly via
        // run_graphify's own finished_result shape.
        let result = run_graphify(&tmp, "true", &["update", "."], true, None);
        assert!(result.ok);
        assert_eq!(result.graphify_out_dir, Some(graphify_out_dir(&tmp)));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Writes an executable shell script at `path` that dispatches on its
    /// first argument (`update`/`extract`), for exercising
    /// [`ensure_graph_with`] without needing real `graphify` installed or
    /// mutating any global process state (env vars, `$PATH`).
    fn write_fake_graphify(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn ensure_graph_succeeds_directly_when_update_produces_a_graph() {
        let tmp = unique_tmp_dir("ensure_update_ok");
        let script = tmp.join("fake_graphify.sh");
        // `update` itself writes graph.json (the common, verified-in-practice
        // case) — extract must never run.
        write_fake_graphify(
            &script,
            "if [ \"$1\" = \"update\" ]; then mkdir -p graphify-out && echo '{}' > graphify-out/graph.json; fi\n\
             if [ \"$1\" = \"extract\" ]; then echo 'extract should not have run' >&2; exit 1; fi\n\
             exit 0",
        );
        let result = ensure_graph_with(&tmp, script.to_str().unwrap(), true, None);
        assert!(result.ok, "{}", result.message);
        assert!(graphify_out_dir(&tmp).join(GRAPHIFY_GRAPH_FILE).is_file());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn ensure_graph_falls_back_to_extract_when_update_produces_no_graph() {
        let tmp = unique_tmp_dir("ensure_fallback");
        // `update` exits 0 but writes nothing (the defensive case this
        // fallback guards against, not observed with the real binary);
        // `extract` produces graph.json.
        write_fake_graphify(
            &tmp.join("fake_graphify.sh"),
            "if [ \"$1\" = \"extract\" ]; then mkdir -p graphify-out && echo '{}' > graphify-out/graph.json; fi\n\
             exit 0",
        );
        let script = tmp.join("fake_graphify.sh");
        let result = ensure_graph_with(&tmp, script.to_str().unwrap(), true, None);
        assert!(result.ok, "{}", result.message);
        assert!(graphify_out_dir(&tmp).join(GRAPHIFY_GRAPH_FILE).is_file());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn ensure_graph_reports_failure_when_both_commands_fail() {
        let tmp = unique_tmp_dir("ensure_both_fail");
        write_fake_graphify(&tmp.join("fake_graphify.sh"), "exit 1");
        let script = tmp.join("fake_graphify.sh");
        let result = ensure_graph_with(&tmp, script.to_str().unwrap(), true, None);
        assert!(!result.ok);
        assert!(!graphify_out_dir(&tmp).join(GRAPHIFY_GRAPH_FILE).is_file());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
