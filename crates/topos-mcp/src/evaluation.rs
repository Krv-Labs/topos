//! Shared evaluation helpers used by the evaluate / assess / inspect tools.
//!
//! Keeps the core pipeline in one place:
//!
//! 1. Build a `ProgramMorphism`.
//! 2. Attach CFG / academic PDG / CPG / Abstractness (always — they're
//!    derived from the morphism itself and require no external tooling).
//! 3. Optionally attach a module-level `ModuleDependencyGraph` from
//!    GitNexus.
//! 4. Call `CharacteristicMorphism::classify_detailed`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde_json::Value;
use topos_core::adapters::gitnexus::{
    current_git_branch, resolve_lbug_store, source_fingerprint, GITNEXUS_FINGERPRINT_FILE,
};
use topos_core::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_core::core::morphism::ProgramMorphism;
use topos_core::evaluation::policies::base::Priority;
use topos_core::functors::probes::uast::abstractness::AbstractnessRepresentation;
use topos_core::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};
use topos_core::graphs::base::Representation;
use topos_core::graphs::mdg::object::{MdgError, ModuleDependencyGraph};

/// Stable prefixes shared by the producer (this module) and the
/// agent-contract consumer (`formatting::composable_contract_signals`) so an
/// invalid/denied override is matched on a single marker.
pub const INVALID_GITNEXUS_MARKERS: [&str; 2] =
    ["gitnexus_dir rejected", "gitnexus_dir unavailable"];

/// Marker inside a "COMPOSABLE not scored" warning meaning the currently
/// checked-out branch has no indexed store.
pub const BRANCH_NOT_INDEXED_MARKER: &str = "no gitnexus store indexed for branch";

/// Stable prefix for staleness warnings.
pub const STALE_GITNEXUS_MARKER: &str = "gitnexus index may be stale";

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

/// Return the gitnexus dir to use, or None if not available.
///
/// Preference: explicit override > `<project_root>/.gitnexus` if it exists.
pub fn resolve_gitnexus_dir(override_dir: Option<&str>, project_root: &Path) -> Option<PathBuf> {
    if let Some(raw) = override_dir {
        let path = PathBuf::from(raw);
        let path = if path.is_absolute() {
            path
        } else {
            project_root.join(path)
        };
        let path = path.canonicalize().ok()?;
        if !path.starts_with(project_root) {
            return None;
        }
        return path.exists().then_some(path);
    }
    let default = project_root.join(".gitnexus");
    default.exists().then_some(default)
}

fn check_override_warning(override_dir: &str, project_root: &Path) -> Option<Vec<String>> {
    let path = PathBuf::from(override_dir);
    let joined = if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    };
    let resolved = joined.canonicalize().unwrap_or(joined);
    if !resolved.starts_with(project_root) {
        return Some(vec![format!(
            "{} — override must be inside TOPOS_MCP_FILE_ROOT. Got: {}",
            INVALID_GITNEXUS_MARKERS[0],
            resolved.display()
        )]);
    }
    if !resolved.exists() {
        return Some(vec![format!(
            "{} — override path does not exist. Got: {}",
            INVALID_GITNEXUS_MARKERS[1],
            resolved.display()
        )]);
    }
    None
}

/// Whether a dep-graph load error is a storage/schema version mismatch.
fn is_schema_mismatch(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["storage version", "different version", "ladybug"]
        .iter()
        .any(|term| lower.contains(term))
}

fn is_branch_not_indexed(message: &str) -> bool {
    message.to_lowercase().contains(BRANCH_NOT_INDEXED_MARKER)
}

fn dep_graph_load_warning(load_error: Option<&str>) -> Vec<String> {
    match load_error {
        Some(err) if is_branch_not_indexed(err) => {
            vec![format!("COMPOSABLE not scored — {err}")]
        }
        Some(err) if is_schema_mismatch(err) => vec![format!(
            "COMPOSABLE not scored — LadybugDB storage version mismatch: {err}"
        )],
        _ => vec![
            "COMPOSABLE not scored — .gitnexus exists but the dependency graph could not \
             be loaded; re-run 'topos depgraph generate' and ensure GitNexus dependencies \
             are installed."
                .to_string(),
        ],
    }
}

/// Explain why COMPOSABLE is unavailable or risky.
pub fn gitnexus_warnings(
    override_dir: Option<&str>,
    project_root: &Path,
    gitnexus_dir: Option<&Path>,
    dep_graph_loaded: bool,
    load_error: Option<&str>,
) -> Vec<String> {
    if let Some(raw) = override_dir {
        if let Some(warn) = check_override_warning(raw, project_root) {
            return warn;
        }
    } else if gitnexus_dir.is_none() {
        return vec![
            "COMPOSABLE not scored — no .gitnexus directory found; run 'topos depgraph \
             generate' to score this generator."
                .to_string(),
        ];
    }

    let mut warnings = Vec::new();
    if let Some(dir) = gitnexus_dir {
        if !dep_graph_loaded {
            warnings.extend(dep_graph_load_warning(load_error));
        }
        if let (_, Some(detail)) = graph_freshness(project_root, dir) {
            warnings.push(detail);
        }
    }
    warnings
}

/// Topos-owned generation marker: what and when the graph was built from.
#[derive(Debug, Clone, Default)]
struct GraphFingerprint {
    head_sha: Option<String>,
    generated_at: Option<f64>,
    finished_at: Option<f64>,
    source_hash: Option<String>,
}

fn read_graph_fingerprint(store_dir: &Path) -> Option<GraphFingerprint> {
    let raw = std::fs::read_to_string(store_dir.join(GITNEXUS_FINGERPRINT_FILE)).ok()?;
    let payload: Value = serde_json::from_str(&raw).ok()?;
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

fn mtime_f64(path: &Path) -> Option<f64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
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
    let paths = topos_core::adapters::discovery::iter_source_files(
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
        if let Some(source_hash) = &fp.source_hash {
            let current = source_fingerprint(project_root);
            if &current.content_hash != source_hash {
                return (
                    true,
                    Some(format!(
                        "{STALE_GITNEXUS_MARKER} — source tree content changed since the \
                         dependency graph was generated; run 'topos depgraph generate' \
                         before trusting COMPOSABLE."
                    )),
                );
            }
            return (false, None);
        }
    }

    let graph_sha = fingerprint.as_ref().and_then(|f| f.head_sha.clone());
    let head_sha = git_head_sha(project_root);
    let sha_anchored = graph_sha.is_some() && head_sha.is_some();
    if let (Some(graph_sha), Some(head_sha)) = (&graph_sha, &head_sha) {
        if graph_sha != head_sha {
            return (
                true,
                Some(format!(
                    "{STALE_GITNEXUS_MARKER} — graph was built from commit {} but HEAD is \
                     {}; run 'topos depgraph generate' before trusting COMPOSABLE.",
                    &graph_sha[..7.min(graph_sha.len())],
                    &head_sha[..7.min(head_sha.len())]
                )),
            );
        }
    }

    if let Some(fp) = &fingerprint {
        if let Some(generated_at) = fp.generated_at {
            let fingerprint_mtime = mtime_f64(&store_dir.join(GITNEXUS_FINGERPRINT_FILE));
            if let Some(newer) = newer_source_file(
                project_root,
                generated_at,
                fp.finished_at,
                fingerprint_mtime,
            ) {
                let rel = newer
                    .strip_prefix(project_root)
                    .unwrap_or(&newer)
                    .to_path_buf();
                return (
                    true,
                    Some(format!(
                        "{STALE_GITNEXUS_MARKER} — {} was modified after the dependency \
                         graph was generated; run 'topos depgraph generate' before \
                         trusting COMPOSABLE.",
                        rel.display()
                    )),
                );
            }
            return (false, None);
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

/// Cheap mtime signal for the gitnexus snapshot, used as a cache key.
pub fn gitnexus_mtime(gitnexus_dir: &Path, current_branch: Option<&str>) -> Option<f64> {
    let resolved = resolve_lbug_store(gitnexus_dir, current_branch);
    match resolved.path {
        Some(lbug) if lbug.exists() => mtime_f64(&lbug),
        _ => mtime_f64(gitnexus_dir),
    }
}

fn resolve_ref_mtime(git_dir: &Path, ref_line: &str) -> Option<f64> {
    let ref_path = git_dir.join(ref_line.strip_prefix("ref: ").unwrap_or(ref_line).trim());
    mtime_f64(&ref_path)
}

/// mtime of the HEAD ref (or HEAD itself when detached).
pub fn git_head_mtime(project_root: &Path) -> Option<f64> {
    let git_dir = project_root.join(".git");
    let head = git_dir.join("HEAD");
    let head_text = std::fs::read_to_string(&head).ok()?;
    let head_text = head_text.trim();
    if head_text.starts_with("ref: ") {
        resolve_ref_mtime(&git_dir, head_text)
    } else {
        mtime_f64(&head)
    }
}

fn packed_ref_sha(git_dir: &Path, git_ref: &str) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name.trim() == git_ref {
            return Some(sha.trim().to_string());
        }
    }
    None
}

/// Current HEAD commit SHA, read from `.git` without shelling out.
pub fn git_head_sha(project_root: &Path) -> Option<String> {
    let git_dir = project_root.join(".git");
    let head_text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head_text = head_text.trim();
    let Some(git_ref) = head_text.strip_prefix("ref: ") else {
        // Detached HEAD: contents are the SHA.
        return (!head_text.is_empty()).then(|| head_text.to_string());
    };
    let git_ref = git_ref.trim();
    match std::fs::read_to_string(git_dir.join(git_ref)) {
        Ok(sha) => {
            let sha = sha.trim().to_string();
            (!sha.is_empty()).then_some(sha)
        }
        Err(_) => packed_ref_sha(&git_dir, git_ref),
    }
}

// ---------------------------------------------------------------------------
// Dep-graph loading (with a small process-local cache)
// ---------------------------------------------------------------------------

type DepGraphCacheKey = (String, String, Option<String>, u64);

static DEP_GRAPH_CACHE: Mutex<Option<HashMap<DepGraphCacheKey, ModuleDependencyGraph>>> =
    Mutex::new(None);

/// Clear the dep-graph cache (primarily for tests).
pub fn clear_caches() {
    if let Ok(mut guard) = DEP_GRAPH_CACHE.lock() {
        *guard = None;
    }
}

fn load_mdg_branch_aware(
    gitnexus_dir: &Path,
    target_file: &str,
    branch: Option<&str>,
) -> Result<ModuleDependencyGraph, String> {
    let resolved = resolve_lbug_store(gitnexus_dir, branch);
    match resolved.path {
        Some(lbug) if lbug.is_dir() => {
            ModuleDependencyGraph::from_json_dir(&lbug, target_file).map_err(|e| e.to_string())
        }
        Some(lbug) if lbug.is_file() => Err(MdgError::LadybugBinaryUnsupported(lbug).to_string()),
        _ => {
            if !resolved.available_branches.is_empty() {
                Err(format!(
                    "{BRANCH_NOT_INDEXED_MARKER} '{}' (indexed: {})",
                    branch.unwrap_or("<detached>"),
                    resolved.available_branches.join(", ")
                ))
            } else {
                Err(MdgError::NotFound(gitnexus_dir.join("lbug")).to_string())
            }
        }
    }
}

/// Load a cached `ModuleDependencyGraph` for the given gitnexus dir + file.
///
/// Returns `(graph, load_error)` — exactly one is `Some`. The cache key
/// includes the store mtime and branch so a GitNexus re-run or branch
/// switch invalidates automatically.
pub fn load_dep_graph(
    gitnexus_dir: Option<&Path>,
    target_file: &str,
) -> (Option<ModuleDependencyGraph>, Option<String>) {
    let Some(gitnexus_dir) = gitnexus_dir else {
        return (None, None);
    };
    let gitnexus_dir = gitnexus_dir
        .canonicalize()
        .unwrap_or_else(|_| gitnexus_dir.to_path_buf());
    let branch = gitnexus_dir.parent().and_then(current_git_branch);
    let mtime_bits = gitnexus_mtime(&gitnexus_dir, branch.as_deref())
        .unwrap_or(0.0)
        .to_bits();
    let key: DepGraphCacheKey = (
        gitnexus_dir.to_string_lossy().to_string(),
        target_file.to_string(),
        branch.clone(),
        mtime_bits,
    );

    if let Ok(mut guard) = DEP_GRAPH_CACHE.lock() {
        let cache = guard.get_or_insert_with(HashMap::new);
        if let Some(graph) = cache.get(&key) {
            return (Some(graph.clone()), None);
        }
    }

    match load_mdg_branch_aware(&gitnexus_dir, target_file, branch.as_deref()) {
        Ok(graph) => {
            if let Ok(mut guard) = DEP_GRAPH_CACHE.lock() {
                let cache = guard.get_or_insert_with(HashMap::new);
                if cache.len() >= 32 {
                    cache.clear();
                }
                cache.insert(key, graph.clone());
            }
            (Some(graph), None)
        }
        Err(err) => (None, Some(err)),
    }
}

// ---------------------------------------------------------------------------
// Depgraph status (read-only)
// ---------------------------------------------------------------------------

/// Structured `.gitnexus` state for the depgraph status MCP tool.
#[derive(Debug, Clone)]
pub struct DepgraphStatus {
    /// missing | present | stale | load_error | schema_mismatch |
    /// invalid_dir | branch_not_indexed
    pub state: &'static str,
    pub gitnexus_dir: Option<String>,
    pub gitnexus_mtime: Option<f64>,
    pub git_head_mtime: Option<f64>,
    pub detail: Option<String>,
}

/// Report `.gitnexus` availability/freshness without shelling out.
pub fn depgraph_status(
    override_dir: Option<&str>,
    project_root: &Path,
    target_file: &str,
) -> DepgraphStatus {
    if let Some(raw) = override_dir {
        if let Some(warn) = check_override_warning(raw, project_root) {
            return DepgraphStatus {
                state: "invalid_dir",
                gitnexus_dir: None,
                gitnexus_mtime: None,
                git_head_mtime: None,
                detail: warn.into_iter().next(),
            };
        }
    }

    let Some(gitnexus_dir) = resolve_gitnexus_dir(override_dir, project_root) else {
        return DepgraphStatus {
            state: "missing",
            gitnexus_dir: None,
            gitnexus_mtime: None,
            git_head_mtime: None,
            detail: Some("No .gitnexus directory found; run topos_generate_depgraph.".into()),
        };
    };

    let branch = current_git_branch(project_root);
    let graph_mtime = gitnexus_mtime(&gitnexus_dir, branch.as_deref());
    let head_mtime = git_head_mtime(project_root);
    let dir_str = gitnexus_dir.to_string_lossy().to_string();

    if let Err(msg) = load_mdg_branch_aware(&gitnexus_dir, target_file, branch.as_deref()) {
        let state = if is_branch_not_indexed(&msg) {
            "branch_not_indexed"
        } else if is_schema_mismatch(&msg) {
            "schema_mismatch"
        } else {
            "load_error"
        };
        return DepgraphStatus {
            state,
            gitnexus_dir: Some(dir_str),
            gitnexus_mtime: graph_mtime,
            git_head_mtime: head_mtime,
            detail: Some(msg),
        };
    }

    let (stale, detail) = graph_freshness(project_root, &gitnexus_dir);
    DepgraphStatus {
        state: if stale { "stale" } else { "present" },
        gitnexus_dir: Some(dir_str),
        gitnexus_mtime: graph_mtime,
        git_head_mtime: head_mtime,
        detail,
    }
}

// ---------------------------------------------------------------------------
// Classification helpers
// ---------------------------------------------------------------------------

/// Run the classifier with CFG/PDG/CPG/Abstractness plus an optional MDG.
pub fn classify_morphism(
    morphism: &mut ProgramMorphism,
    priority: Priority,
    dep_graph: Option<&ModuleDependencyGraph>,
) -> ClassificationResult {
    let cfg = morphism.build_cfg().cloned();
    let pdg = morphism.build_pdg().cloned();
    let cpg = morphism.build_cpg().cloned();
    let abstractness = morphism
        .ast
        .as_ref()
        .map(|ast| AbstractnessRepresentation::new(&ast.uast_root));

    let mut representations: Vec<&dyn Representation> = Vec::new();
    if let Some(cfg) = &cfg {
        representations.push(cfg);
    }
    if let Some(pdg) = &pdg {
        representations.push(pdg);
    }
    if let Some(cpg) = &cpg {
        representations.push(cpg);
    }
    if let Some(abstractness) = &abstractness {
        representations.push(abstractness);
    }
    if let Some(dep_graph) = dep_graph {
        representations.push(dep_graph);
    }

    CharacteristicMorphism.classify_detailed(morphism, &representations, priority)
}

/// Classify raw source. CFG / PDG / CPG always run; the COMPOSABLE
/// generator is unreachable without a ModuleDependencyGraph.
pub fn classify_code_string(
    code: &str,
    language: &str,
    priority: Priority,
) -> Result<ClassificationResult, String> {
    if !SUPPORTED_LANGUAGES.contains(&language) {
        let mut expected: Vec<&str> = SUPPORTED_LANGUAGES.to_vec();
        expected.sort_unstable();
        return Err(format!(
            "Unsupported language '{language}'; expected one of {expected:?}"
        ));
    }
    let mut morphism = ProgramMorphism::new(code, language);
    Ok(classify_morphism(&mut morphism, priority, None))
}

/// Map a file suffix to a tree-sitter language, defaulting to `python`.
pub fn detect_language(path: &Path) -> &'static str {
    let suffix = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for lang in SUPPORTED_LANGUAGES {
        if let Some(suffixes) = language_file_suffixes(lang) {
            if suffixes.contains(&suffix.as_str()) {
                return lang;
            }
        }
    }
    "python"
}

/// Classify a file, attaching every available representation.
///
/// Returns `(result, dep_graph, load_error)` so callers can cache the dep
/// graph for subsequent proposed-code evaluations.
#[allow(clippy::type_complexity)]
pub fn classify_file(
    path: &Path,
    priority: Priority,
    gitnexus_dir: Option<&Path>,
) -> Result<
    (
        ClassificationResult,
        Option<ModuleDependencyGraph>,
        Option<String>,
    ),
    String,
> {
    let language = detect_language(path);
    let mut morphism = ProgramMorphism::from_file(path, language)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (dep_graph, load_error) = load_dep_graph(gitnexus_dir, &path.to_string_lossy());
    let result = classify_morphism(&mut morphism, priority, dep_graph.as_ref());
    Ok((result, dep_graph, load_error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_by_suffix() {
        assert_eq!(detect_language(Path::new("a.rs")), "rust");
        assert_eq!(detect_language(Path::new("a.py")), "python");
        assert_eq!(detect_language(Path::new("a.tsx")), "typescript");
        assert_eq!(detect_language(Path::new("a.unknown")), "python");
    }

    #[test]
    fn classify_code_string_rejects_unknown_language() {
        assert!(classify_code_string("x = 1", "cobol", Priority::Simple).is_err());
    }

    #[test]
    fn classify_code_string_scores_python() {
        let result = classify_code_string("def f():\n    return 1\n", "python", Priority::Simple)
            .expect("classification runs");
        assert!(result.is_parseable);
        assert!(result.scores.contains_key("simple"));
    }
}
