//! Locked `.gitnexus` dependency-graph generation and freshness-ensure helpers.
//!
//! Serializes concurrent `gitnexus analyze` invocations targeting the same
//! project root via [`graph_build_lock`], propagates cooperative cancellation
//! ([`EvalCancelled`]), and invalidates both the in-memory `ModuleDependencyGraph`
//! cache and the worktree freshness fingerprint cache after every build attempt.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use topos_engine::adapters::gitnexus::{
    generate_depgraph_with_cancel, gitnexus_available, resolve_timeout, DepgraphGenerationResult,
    CANCELLED_RC, DEFAULT_ANALYZE_TIMEOUT_S,
};

use super::{
    clear_caches, clear_freshness_cache, depgraph_status, generation_failure_note,
    resolve_gitnexus_dir,
};
use crate::execution::{EvalCancelled, StatusReporter};

static GRAPH_BUILD_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[inline]
fn cancelled(cancel_flag: Option<&AtomicBool>) -> bool {
    cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed))
}

/// Return the per-target-directory mutex serializing concurrent `gitnexus analyze`
/// builds against the same project/store root.
pub fn graph_build_lock(target_dir: &Path) -> Arc<Mutex<()>> {
    let key = target_dir
        .canonicalize()
        .unwrap_or_else(|_| target_dir.to_path_buf());
    let mut map = GRAPH_BUILD_LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(map.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))))
}

/// Wait for a graph build without retaining a canceled worker behind its owner.
fn acquire_build_lock<'a>(
    lock: &'a Mutex<()>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<MutexGuard<'a, ()>, EvalCancelled> {
    let Some(cancel_flag) = cancel_flag else {
        return Ok(lock.lock().unwrap_or_else(|p| p.into_inner()));
    };
    loop {
        if cancelled(Some(cancel_flag)) {
            return Err(EvalCancelled);
        }
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(p)) => return Ok(p.into_inner()),
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn analyze_execution_timeout(timeout_s: Option<f64>) -> Duration {
    timeout_s
        .filter(|s| s.is_finite())
        .and_then(|s| Duration::try_from_secs_f64(s.max(1.0)).ok())
        .unwrap_or_else(|| Duration::from_secs_f64(DEFAULT_ANALYZE_TIMEOUT_S))
}

/// Outcome of a serialized `topos_generate_depgraph` execution.
pub enum LockedGenerateOutcome {
    /// Another concurrent caller already refreshed the graph while we waited
    /// for the per-root build lock.
    AlreadyCurrent { gitnexus_dir: Option<String> },
    /// `gitnexus analyze` was executed while holding the per-root build lock.
    Generated(DepgraphGenerationResult),
}

/// Verify that `gitnexus` is on `$PATH` and return the configured analyze timeout,
/// or the immediate `not found` generation result when `gitnexus` is unavailable.
pub fn depgraph_analyze_timeout(target_dir: &Path) -> Result<Duration, DepgraphGenerationResult> {
    if !gitnexus_available() {
        return Err(generate_depgraph_with_cancel(target_dir, true, None, None));
    }
    Ok(analyze_execution_timeout(resolve_timeout(None)))
}

/// Acquire the per-target-directory build lock, re-check freshness when `!force`,
/// run `gitnexus analyze` with cooperative cancellation, and invalidate caches.
pub fn run_locked_depgraph_generation(
    target_dir: &Path,
    status_override: Option<&str>,
    force: bool,
    cancel_flag: Option<&AtomicBool>,
    status_reporter: Option<&StatusReporter>,
) -> Result<LockedGenerateOutcome, EvalCancelled> {
    let lock = graph_build_lock(target_dir);
    let _guard = acquire_build_lock(&lock, cancel_flag)?;

    if cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    if !force {
        let status = depgraph_status(status_override, target_dir, &target_dir.to_string_lossy());
        if status.state == "present" {
            if let Some(ref dir) = status.gitnexus_dir {
                crate::cache::graph_used(Path::new(dir));
            }
            return Ok(LockedGenerateOutcome::AlreadyCurrent {
                gitnexus_dir: status.gitnexus_dir,
            });
        }
    }

    if let Some(reporter) = status_reporter {
        reporter.report("Running gitnexus analyze");
    }

    let result = generate_depgraph_with_cancel(target_dir, true, None, cancel_flag);
    clear_caches();
    clear_freshness_cache();

    if result.returncode == CANCELLED_RC || cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    if result.ok {
        if let Some(ref dir) = result.gitnexus_path {
            crate::cache::graph_used(dir);
        }
    }

    Ok(LockedGenerateOutcome::Generated(result))
}

/// Outcome of [`ensure_gitnexus_dir`]: the dir to attach (if any), plus a
/// note describing a generation attempt — `Some` only when generation was
/// attempted and didn't result in a usable graph (not found on `$PATH`, or
/// the `gitnexus analyze` run itself failed). `None` in every other case
/// (already present/fresh, skipped, or generation succeeded), since
/// [`super::gitnexus_warnings`] already explains any remaining unavailability
/// (invalid override, schema mismatch, branch not indexed, ...) from the
/// resulting `gitnexus_dir` state.
pub struct GitnexusEnsureOutcome {
    pub gitnexus_dir: Option<PathBuf>,
    pub generation_note: Option<String>,
}

/// Resolve the gitnexus dir to attach for COMPOSABLE, generating/refreshing
/// it first when missing or stale — the shared "ensure" decision behind
/// both the CLI's default `topos evaluate` and the MCP evaluate tools'
/// default behavior, so the two standardize on one "always try to score all
/// four pillars" policy.
pub fn ensure_gitnexus_dir(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
) -> GitnexusEnsureOutcome {
    ensure_gitnexus_dir_with_progress(override_dir, project_root, skip, capture, &mut |_| {})
}

fn state_needs_rebuild(state: &str) -> bool {
    matches!(
        state,
        "missing" | "stale" | "branch_not_indexed" | "load_error"
    )
}

fn rebuild_gitnexus_locked(
    override_dir: Option<&str>,
    project_root: &Path,
    capture: bool,
    cancel_flag: Option<&AtomicBool>,
    progress: &mut dyn FnMut(&'static str),
) -> Result<Option<String>, EvalCancelled> {
    let lock = graph_build_lock(project_root);
    let _guard = acquire_build_lock(&lock, cancel_flag)?;
    if cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let rechecked = depgraph_status(override_dir, project_root, &project_root.to_string_lossy());
    if cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    if !state_needs_rebuild(rechecked.state) {
        return Ok(None);
    }
    progress("Running gitnexus analyze");
    let result = generate_depgraph_with_cancel(project_root, capture, None, cancel_flag);
    clear_caches();
    clear_freshness_cache();
    if result.returncode == CANCELLED_RC || cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    if result.ok {
        if let Some(ref dir) = result.gitnexus_path {
            crate::cache::graph_used(dir);
        }
    }
    Ok((!result.ok).then(|| generation_failure_note(&result.message)))
}

/// Same as [`ensure_gitnexus_dir`], invoking `progress` with stable phase
/// labels so CLI spinners can distinguish freshness checks from
/// `gitnexus analyze`.
pub fn ensure_gitnexus_dir_with_progress(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
    progress: &mut dyn FnMut(&'static str),
) -> GitnexusEnsureOutcome {
    ensure_gitnexus_dir_inner(override_dir, project_root, skip, capture, None, progress).unwrap_or(
        GitnexusEnsureOutcome {
            gitnexus_dir: None,
            generation_note: None,
        },
    )
}

/// Cancellation-aware and lock-serialized variant of [`ensure_gitnexus_dir`]
/// for MCP tool workers.
///
/// Propagates [`EvalCancelled`] as a distinct error rather than converting an
/// aborted `gitnexus analyze` subprocess into a non-fatal `generation_note`.
pub fn ensure_gitnexus_dir_with_cancel(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
    cancel_flag: Option<&AtomicBool>,
    status_reporter: Option<&StatusReporter>,
) -> Result<GitnexusEnsureOutcome, EvalCancelled> {
    let mut progress = |msg: &'static str| {
        if let Some(reporter) = status_reporter {
            reporter.report(msg);
        }
    };
    ensure_gitnexus_dir_inner(
        override_dir,
        project_root,
        skip,
        capture,
        cancel_flag,
        &mut progress,
    )
}

fn ensure_gitnexus_dir_inner(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
    cancel_flag: Option<&AtomicBool>,
    progress: &mut dyn FnMut(&'static str),
) -> Result<GitnexusEnsureOutcome, EvalCancelled> {
    if cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    if skip {
        return Ok(GitnexusEnsureOutcome {
            gitnexus_dir: None,
            generation_note: None,
        });
    }
    let _cache_lease = match crate::cache::lease_managed_path(project_root) {
        Ok(lease) => lease,
        Err(error) => {
            return Ok(GitnexusEnsureOutcome {
                gitnexus_dir: None,
                generation_note: Some(format!(
                    "PR cache is in use by another Topos operation ({error}); \
                     COMPOSABLE not scored."
                )),
            });
        }
    };
    progress("Checking dependency graph freshness");
    let status = depgraph_status(override_dir, project_root, &project_root.to_string_lossy());
    if cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    if !state_needs_rebuild(status.state) {
        return Ok(GitnexusEnsureOutcome {
            gitnexus_dir: resolve_gitnexus_dir(override_dir, project_root),
            generation_note: None,
        });
    }
    if !gitnexus_available() {
        return Ok(GitnexusEnsureOutcome {
            gitnexus_dir: resolve_gitnexus_dir(override_dir, project_root),
            generation_note: Some(
                "GitNexus not found on $PATH — COMPOSABLE not scored. Install it with \
                 `npm install -g gitnexus` to enable COMPOSABLE."
                    .to_string(),
            ),
        });
    }
    let generation_note =
        rebuild_gitnexus_locked(override_dir, project_root, capture, cancel_flag, progress)?;
    Ok(GitnexusEnsureOutcome {
        gitnexus_dir: resolve_gitnexus_dir(override_dir, project_root),
        generation_note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canceled_build_waiter_exits_while_owner_still_holds_lock() {
        let lock = Arc::new(Mutex::new(()));
        let owner = lock.lock().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker_lock = Arc::clone(&lock);
        let worker_cancel = Arc::clone(&cancel);
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = acquire_build_lock(&worker_lock, Some(&worker_cancel));
            done_tx.send(result.is_err()).unwrap();
        });
        started_rx.recv().unwrap();
        cancel.store(true, Ordering::SeqCst);
        let result = done_rx.recv_timeout(Duration::from_secs(1));
        // Release the owner even on failure, so a broken waiter cannot hang the test.
        drop(owner);
        worker.join().unwrap();
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn analyze_timeout_handles_nonfinite_and_overflowing_values() {
        let default = Duration::from_secs_f64(DEFAULT_ANALYZE_TIMEOUT_S);
        for value in [None, Some(f64::INFINITY), Some(f64::NAN), Some(1e300)] {
            assert_eq!(analyze_execution_timeout(value), default);
        }
        assert_eq!(analyze_execution_timeout(Some(0.5)), Duration::from_secs(1));
        assert_eq!(
            analyze_execution_timeout(Some(2.5)),
            Duration::from_millis(2500)
        );
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_eval_build_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_gitnexus_dir_skip_ignores_an_existing_store_without_shelling_out() {
        let project_root = temp_dir("skip_root");
        std::fs::create_dir(project_root.join(".gitnexus")).unwrap();
        let outcome = ensure_gitnexus_dir(None, &project_root, true, false);
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome.generation_note.is_none());
        std::fs::remove_dir_all(&project_root).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_returns_none_for_override_outside_project_root_without_shelling_out() {
        let project_root = temp_dir("invalid_root");
        let outside = temp_dir("invalid_outside");

        let outcome = ensure_gitnexus_dir(
            Some(&outside.to_string_lossy()),
            &project_root,
            false,
            false,
        );
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome.generation_note.is_none());
        let status = depgraph_status(
            Some(&outside.to_string_lossy()),
            &project_root,
            &project_root.to_string_lossy(),
        );
        assert_eq!(status.state, "invalid_dir");

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_degrades_gracefully_when_gitnexus_missing_from_path() {
        if gitnexus_available() {
            return;
        }
        let project_root = temp_dir("missing_no_gitnexus");
        let outcome = ensure_gitnexus_dir(None, &project_root, false, true);
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome
            .generation_note
            .as_deref()
            .is_some_and(|n| n.contains("not found on $PATH")));
        std::fs::remove_dir_all(&project_root).ok();
    }
}
