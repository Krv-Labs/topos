//! Graph freshness / fingerprint checking against the working tree.

use std::path::{Path, PathBuf};

use serde_json::Value;
use topos_engine::adapters::gitnexus::{
    current_git_branch, resolve_lbug_store, source_fingerprint, GITNEXUS_FINGERPRINT_FILE,
};
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

use super::gitref::{git_head_mtime, git_head_sha, gitnexus_mtime, mtime_f64};
use super::STALE_GITNEXUS_MARKER;

/// Freshness stat-walk ceiling: beyond this many source files the mtime
/// pass returns "fresh" rather than making every evaluate call pay for a
/// pathological monorepo walk. The SHA anchor still catches commit-level
/// drift there.
const FRESHNESS_WALK_CAP: usize = 20_000;

/// Tolerance for the mtime-vs-generated_at comparison (see the Python
/// original's rationale: second-truncated mtimes and clock drift).
const MTIME_SKEW_TOLERANCE_S: f64 = 2.0;

/// Sanity bound on the measured (finished_at - generated_at) duration used
/// to calibrate filesystem-clock drift.
const MAX_TRUSTED_GENERATION_DURATION_S: f64 = 3600.0;

/// Topos-owned generation marker: what and when the graph was built from.
#[derive(Debug, Clone, Default)]
struct GraphFingerprint {
    head_sha: Option<String>,
    generated_at: Option<f64>,
    finished_at: Option<f64>,
    source_hash: Option<String>,
}

fn read_graph_fingerprint(store_dir: &Path) -> Option<GraphFingerprint> {
    // Named `raw_json`, not `raw`: tree-sitter-rust's grammar for the
    // `&raw const`/`&raw mut` raw-reference syntax misparses a bare `&raw`
    // local as an ERROR node, which would falsely mark this whole file
    // unparseable (is_parseable=false) despite valid, rustc-accepted syntax.
    let raw_json = std::fs::read_to_string(store_dir.join(GITNEXUS_FINGERPRINT_FILE)).ok()?;
    let payload: Value = serde_json::from_str(&raw_json).ok()?;
    Some(GraphFingerprint {
        head_sha: payload
            .get("head_sha")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        generated_at: payload.get("generated_at").and_then(Value::as_f64),
        finished_at: payload.get("finished_at").and_then(Value::as_f64),
        source_hash: payload
            .get("source_hash")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// All supported source suffixes, deduped.
pub fn all_source_suffixes() -> Vec<&'static str> {
    let mut all: Vec<&str> = SUPPORTED_LANGUAGES
        .iter()
        .filter_map(|lang| language_file_suffixes(lang))
        .flat_map(|group| group.iter().copied())
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// First source file (or directory) modified after `generated_at`, or None.
fn newer_source_file(
    project_root: &Path,
    generated_at: f64,
    finished_at: Option<f64>,
    fingerprint_mtime: Option<f64>,
) -> Option<PathBuf> {
    let suffixes = all_source_suffixes();
    let mut threshold = generated_at - MTIME_SKEW_TOLERANCE_S;
    if let (Some(finished), Some(fp_mtime)) = (finished_at, fingerprint_mtime) {
        let duration = finished - generated_at;
        if (0.0..=MAX_TRUSTED_GENERATION_DURATION_S).contains(&duration) {
            if fp_mtime.fract() == 0.0 {
                let drift = finished.trunc() - fp_mtime;
                threshold = generated_at.trunc() - drift;
            } else {
                let drift = finished - fp_mtime;
                threshold = generated_at - drift;
            }
        }
    }
    let paths = topos_engine::adapters::discovery::iter_source_files(
        project_root,
        &suffixes,
        true,
        None,
        true, // include_dirs: deletions bump the parent dir's mtime
    );
    let mut seen = 0usize;
    for path in paths {
        let is_file = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| suffixes.contains(&format!(".{e}").as_str()))
            .unwrap_or(false);
        if is_file {
            seen += 1;
            if seen > FRESHNESS_WALK_CAP {
                return None;
            }
        }
        if let Some(mtime) = mtime_f64(&path) {
            if mtime > threshold {
                return Some(path);
            }
        }
    }
    None
}

fn stale_from_source_hash(
    project_root: &Path,
    fingerprint: &GraphFingerprint,
) -> Option<(bool, Option<String>)> {
    let source_hash = fingerprint.source_hash.as_ref()?;
    let current = source_fingerprint(project_root);
    if &current.content_hash != source_hash {
        return Some((
            true,
            Some(format!(
                "{STALE_GITNEXUS_MARKER} — source tree content changed since the \
                 dependency graph was generated; run 'topos depgraph generate' \
                 before trusting COMPOSABLE."
            )),
        ));
    }
    Some((false, None))
}

fn stale_from_head_sha(
    project_root: &Path,
    fingerprint: &GraphFingerprint,
) -> (bool, Option<(bool, Option<String>)>) {
    let graph_sha = fingerprint.head_sha.as_deref();
    let head_sha = git_head_sha(project_root);
    let sha_anchored = graph_sha.is_some() && head_sha.is_some();
    if let (Some(g), Some(h)) = (graph_sha, head_sha.as_deref()) {
        if g != h {
            return (
                sha_anchored,
                Some((
                    true,
                    Some(format!(
                        "{STALE_GITNEXUS_MARKER} — graph was built from commit {} but HEAD is \
                         {}; run 'topos depgraph generate' before trusting COMPOSABLE.",
                        &g[..7.min(g.len())],
                        &h[..7.min(h.len())]
                    )),
                )),
            );
        }
    }
    (sha_anchored, None)
}

fn stale_from_mtime_walk(
    project_root: &Path,
    store_dir: &Path,
    fingerprint: &GraphFingerprint,
) -> Option<(bool, Option<String>)> {
    let generated_at = fingerprint.generated_at?;
    let fingerprint_mtime = mtime_f64(&store_dir.join(GITNEXUS_FINGERPRINT_FILE));
    if let Some(newer) = newer_source_file(
        project_root,
        generated_at,
        fingerprint.finished_at,
        fingerprint_mtime,
    ) {
        let rel = newer
            .strip_prefix(project_root)
            .unwrap_or(&newer)
            .to_path_buf();
        return Some((
            true,
            Some(format!(
                "{STALE_GITNEXUS_MARKER} — {} was modified after the dependency \
                 graph was generated; run 'topos depgraph generate' before \
                 trusting COMPOSABLE.",
                rel.display()
            )),
        ));
    }
    Some((false, None))
}

/// Whether the dependency graph is stale w.r.t. the working tree.
///
/// Anchors, in preference order: source content hash, commit SHA, the
/// `generated_at` mtime walk, then the legacy dir-mtime comparison.
pub fn graph_freshness(project_root: &Path, gitnexus_dir: &Path) -> (bool, Option<String>) {
    let branch = current_git_branch(project_root);
    let resolved = resolve_lbug_store(gitnexus_dir, branch.as_deref());
    let store_dir = resolved
        .path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(gitnexus_dir)
        .to_path_buf();

    let fingerprint = read_graph_fingerprint(&store_dir);
    if let Some(fp) = &fingerprint {
        if let Some(result) = stale_from_source_hash(project_root, fp) {
            return result;
        }
    }

    let (sha_anchored, sha_mismatch) = fingerprint
        .as_ref()
        .map(|fp| stale_from_head_sha(project_root, fp))
        .unwrap_or((false, None));
    if let Some(result) = sha_mismatch {
        return result;
    }

    if let Some(fp) = &fingerprint {
        if let Some(result) = stale_from_mtime_walk(project_root, &store_dir, fp) {
            return result;
        }
    }

    if sha_anchored {
        // v1 fingerprint (SHA only, matching HEAD): no working-tree signal.
        return (false, None);
    }

    // Legacy fallback: compare the graph DB mtime to the latest commit's.
    let graph_mtime = gitnexus_mtime(gitnexus_dir, branch.as_deref());
    let head_mtime = git_head_mtime(project_root);
    match (graph_mtime, head_mtime) {
        (Some(g), Some(h)) if g > 0.0 && g < h => (
            true,
            Some(format!(
                "{STALE_GITNEXUS_MARKER} — .gitnexus is older than the latest git commit; \
                 run 'topos depgraph generate' before trusting COMPOSABLE."
            )),
        ),
        _ => (false, None),
    }
}
