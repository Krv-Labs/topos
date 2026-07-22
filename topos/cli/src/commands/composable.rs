//! Resolve-or-generate the COMPOSABLE `.gitnexus` dependency graph for
//! `topos evaluate`.
//!
//! Split out of `evaluate.rs` -- this is GitNexus/MDG resolution
//! infrastructure, not `evaluate`-command orchestration.

use std::path::Path;

use topos_engine::adapters::gitnexus::{
    current_git_branch, gitnexus_compat_warning, resolve_lbug_store,
};
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_mcp::evaluation::ensure_gitnexus_dir;

/// Ensure a fresh `.gitnexus` build exists for `project_root` and load its
/// `ModuleDependencyGraph`, or return `None` (with an explanatory `stderr`
/// notice) if that isn't possible. Never returns an `Err` — COMPOSABLE is
/// optional and its absence must not fail the whole evaluate run.
///
/// The resolve-or-generate decision itself (present/missing/stale, GitNexus
/// availability, generation) is shared with the MCP evaluate tools via
/// `topos_mcp::evaluation::ensure_gitnexus_dir` — this function only adds
/// the CLI-specific "load once, reuse across every file in this run" MDG
/// parsing on top (unlike MCP's `load_dep_graph`, which caches per
/// `target_file` — a fit for arbitrary single-file tool calls, but N cache
/// misses across a directory walk of N files).
pub(crate) fn resolve_composable_mdg(
    project_root: &Path,
    gitnexus_dir_override: Option<&str>,
) -> Option<ModuleDependencyGraph> {
    let outcome = ensure_gitnexus_dir(
        gitnexus_dir_override,
        project_root,
        /* skip = */ false,
        /* capture = */ false,
    );
    if let Some(note) = &outcome.generation_note {
        eprintln!("gitnexus: {note}");
    }
    let gitnexus_dir = outcome.gitnexus_dir?;

    if let Some(warn) = gitnexus_compat_warning() {
        eprintln!("gitnexus: {warn}");
    }

    let branch = current_git_branch(project_root);
    let resolved = resolve_lbug_store(&gitnexus_dir, branch.as_deref());
    let Some(lbug_path) = resolved.path else {
        if !resolved.available_branches.is_empty() {
            eprintln!(
                "gitnexus: current branch is not indexed (indexed: {}) — evaluating SIMPLE/SECURE only.",
                resolved.available_branches.join(", ")
            );
        } else {
            eprintln!(
                "gitnexus: no indexed store found at {} — evaluating SIMPLE/SECURE only.",
                gitnexus_dir.display()
            );
        }
        return None;
    };

    if !lbug_path.exists() {
        eprintln!(
            "gitnexus: no indexed store found at {} — evaluating SIMPLE/SECURE only.",
            gitnexus_dir.display()
        );
        return None;
    }

    match ModuleDependencyGraph::from_lbug_path(
        &lbug_path,
        project_root.to_string_lossy(),
        project_root,
        branch.as_deref(),
    ) {
        Ok(graph) => Some(graph),
        Err(e) => {
            eprintln!(
                "gitnexus: failed to load dependency graph ({e}) — evaluating SIMPLE/SECURE only."
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_composable_mdg_returns_none_for_override_outside_project_root_without_shelling_out()
    {
        // An override outside `project_root` is rejected by
        // `resolve_gitnexus_dir`/`depgraph_status` before any
        // availability check or subprocess call, so this stays
        // deterministic regardless of whether gitnexus happens to be
        // installed on the machine running the test.
        let temp_dir = |label: &str| -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "topos_cli_composable_test_{label}_{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let project_root = temp_dir("root");
        let outside = temp_dir("outside");

        let result = resolve_composable_mdg(&project_root, Some(&outside.to_string_lossy()));
        assert!(result.is_none());

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
}
