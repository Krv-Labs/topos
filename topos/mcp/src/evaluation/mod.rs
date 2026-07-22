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

mod classify;
mod depgraph;
mod freshness;
mod gitref;

pub use classify::{classify_code_string, classify_file, classify_morphism, detect_language};
pub use depgraph::{clear_caches, depgraph_status, load_dep_graph, DepgraphStatus};
pub use freshness::{all_source_suffixes, graph_freshness};
pub use gitref::{git_head_mtime, git_head_sha, gitnexus_mtime};

use std::path::{Path, PathBuf};

use topos_engine::adapters::gitnexus::{generate_depgraph, gitnexus_available};

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

/// Outcome of [`ensure_gitnexus_dir`]: the dir to attach (if any), plus a
/// note describing a generation attempt — `Some` only when generation was
/// attempted and didn't result in a usable graph (not found on `$PATH`, or
/// the `gitnexus analyze` run itself failed). `None` in every other case
/// (already present/fresh, skipped, or generation succeeded), since
/// [`gitnexus_warnings`] already explains any remaining unavailability
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
/// three pillars" policy. `skip=true` reproduces the old read-only behavior
/// (just [`resolve_gitnexus_dir`], no generation). `capture` is forwarded to
/// `generate_depgraph`: `false` streams GitNexus's own output to the
/// inherited stdio (the CLI, where a human is watching), `true` collects it
/// into the result instead (MCP, over a stdio transport already carrying
/// the protocol).
///
/// Never blocks indefinitely: `generate_depgraph` bounds the `gitnexus
/// analyze` subprocess with `TOPOS_DEPGRAPH_TIMEOUT` (default 300s).
/// Callers running this synchronously on an async runtime should offload it
/// (e.g. `tokio::task::spawn_blocking`) so a slow/first-time generation on a
/// large repo cannot stall the transport.
pub fn ensure_gitnexus_dir(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
) -> GitnexusEnsureOutcome {
    let resolve = || resolve_gitnexus_dir(override_dir, project_root);
    if skip {
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: None,
        };
    }

    let status = depgraph_status(override_dir, project_root, &project_root.to_string_lossy());
    if !matches!(status.state, "missing" | "stale") {
        // present, or a problem generating won't fix (invalid_dir,
        // schema_mismatch, branch_not_indexed, load_error) — let
        // gitnexus_warnings explain it from the resolved state.
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: None,
        };
    }

    if !gitnexus_available() {
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: Some(
                "GitNexus not found on $PATH — COMPOSABLE not scored. Install it with \
                 `npm install -g gitnexus` to enable COMPOSABLE."
                    .to_string(),
            ),
        };
    }

    let result = generate_depgraph(project_root, capture, None);
    let generation_note = (!result.ok).then(|| {
        format!(
            "GitNexus generation failed ({}) — COMPOSABLE not scored.",
            result.message
        )
    });
    GitnexusEnsureOutcome {
        gitnexus_dir: resolve(),
        generation_note,
    }
}

/// Return the graphify output dir to use, or None if not available.
///
/// Preference: explicit override > Graphify's own default resolution
/// (`topos_engine::adapters::graphify::graphify_out_dir`, which itself honors
/// `GRAPHIFY_OUT`) — so the read side (this function) and the generate side
/// never disagree about where to look.
pub fn resolve_graphify_dir(override_dir: Option<&str>, project_root: &Path) -> Option<PathBuf> {
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
    let default = topos_engine::adapters::graphify::graphify_out_dir(project_root);
    default.exists().then_some(default)
}

pub(crate) fn check_override_warning(
    override_dir: &str,
    project_root: &Path,
) -> Option<Vec<String>> {
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
pub(crate) fn is_schema_mismatch(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["storage version", "different version", "ladybug"]
        .iter()
        .any(|term| lower.contains(term))
}

pub(crate) fn is_branch_not_indexed(message: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use topos_engine::adapters::gitnexus::gitnexus_available;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_evaluation_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_gitnexus_dir_skip_reproduces_plain_resolve_without_shelling_out() {
        // skip=true must behave exactly like the old read-only
        // resolve_gitnexus_dir — no depgraph_status/gitnexus_available/
        // generate_depgraph call at all, so this is deterministic
        // regardless of whether gitnexus happens to be on the test
        // machine's PATH.
        let project_root = temp_dir("skip_root");
        let outcome = ensure_gitnexus_dir(None, &project_root, true, false);
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome.generation_note.is_none());
        std::fs::remove_dir_all(&project_root).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_returns_none_for_override_outside_project_root_without_shelling_out() {
        // An override outside project_root is rejected by
        // resolve_gitnexus_dir/depgraph_status before any availability
        // check or subprocess call — deterministic either way.
        let project_root = temp_dir("invalid_root");
        let outside = temp_dir("invalid_outside");

        let outcome = ensure_gitnexus_dir(
            Some(&outside.to_string_lossy()),
            &project_root,
            false,
            false,
        );
        assert!(outcome.gitnexus_dir.is_none());
        // invalid_dir is a state generation can't fix, so no generation
        // is attempted and no generation_note is set — gitnexus_warnings
        // (fed by depgraph_status separately) is what explains it.
        assert!(outcome.generation_note.is_none());

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_degrades_gracefully_when_gitnexus_missing_from_path() {
        if gitnexus_available() {
            // Dev boxes may have GitNexus installed; skip rather than
            // shell out to the real binary from a unit test.
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
