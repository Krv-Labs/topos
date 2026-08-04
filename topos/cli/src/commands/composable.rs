//! Resolve-or-generate the COMPOSABLE `.gitnexus` dependency graph for
//! `topos evaluate` / `topos inspect`.
//!
//! Split out of `evaluate.rs` — this is GitNexus/MDG resolution
//! infrastructure, not `evaluate`-command orchestration.

use std::path::Path;

use topos_engine::adapters::gitnexus::{
    current_git_branch, gitnexus_compat_warning, resolve_lbug_store,
};
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_mcp::evaluation::{ensure_gitnexus_dir_with_progress, gitnexus_warnings};

/// Result of a CLI COMPOSABLE resolve-or-generate attempt.
///
/// `mdg` is `Some` only when a graph loaded successfully. `warnings` carries
/// the same explanations MCP attaches via [`gitnexus_warnings`] (plus any
/// generation failure note), so human stderr and `--json` stay in parity
/// with the MCP `warnings` field instead of only showing
/// "COMPOSABLE not measured".
#[derive(Debug, Default)]
pub(crate) struct ComposableResolve {
    pub mdg: Option<ModuleDependencyGraph>,
    pub warnings: Vec<String>,
}

/// Ensure a fresh `.gitnexus` build exists for `project_root` and load its
/// `ModuleDependencyGraph`, or return `mdg: None` with explanatory
/// `warnings` if that isn't possible. Never returns an `Err` — COMPOSABLE is
/// optional and its absence must not fail the whole evaluate run.
///
/// The resolve-or-generate decision itself (present/missing/stale, GitNexus
/// availability, generation) is shared with the MCP evaluate tools via
/// `topos_mcp::evaluation::ensure_gitnexus_dir` — this function only adds
/// the CLI-specific "load once, reuse across every file in this run" MDG
/// parsing on top (unlike MCP's `load_dep_graph`, which caches per
/// `target_file` — a fit for arbitrary single-file tool calls, but N cache
/// misses across a directory walk of N files).
///
/// Warnings are returned for `--json` and for the evaluate/inspect summary
/// card (orange `↻` notice). Callers that print a card should not also dump
/// the same strings to stderr — that doubles the noise. Only advisory
/// compatibility nags still print immediately.
///
/// `quiet` captures GitNexus output. Machine-readable callers require this to
/// keep stdout valid; interactive callers can also capture it while presenting
/// their own stable progress indicator.
///
/// `progress` receives phase labels (`Checking dependency graph freshness`,
/// `Running gitnexus analyze`) for spinner updates.
pub(crate) fn resolve_composable_mdg(
    project_root: &Path,
    gitnexus_dir_override: Option<&str>,
    quiet: bool,
    progress: &mut dyn FnMut(&'static str),
) -> ComposableResolve {
    let outcome = ensure_gitnexus_dir_with_progress(
        gitnexus_dir_override,
        project_root,
        /* skip = */ false,
        /* capture = */ quiet,
        progress,
    );

    let mut warnings = Vec::new();
    if let Some(note) = outcome.generation_note {
        warnings.push(note);
    }

    let Some(gitnexus_dir) = outcome.gitnexus_dir else {
        warnings.extend(gitnexus_warnings(
            gitnexus_dir_override,
            project_root,
            None,
            false,
            None,
        ));
        return ComposableResolve {
            mdg: None,
            warnings,
        };
    };

    if let Some(warn) = gitnexus_compat_warning() {
        // Compat is advisory — keep it on stderr only so JSON consumers that
        // key off setup failures are not flooded with version nags.
        eprintln!("gitnexus: {warn}");
    }

    let branch = current_git_branch(project_root);
    let resolved = resolve_lbug_store(&gitnexus_dir, branch.as_deref());
    let Some(lbug_path) = resolved.path else {
        let load_error = if !resolved.available_branches.is_empty() {
            format!(
                "no gitnexus store indexed for branch {}; indexed: {}",
                branch.as_deref().unwrap_or("(unknown)"),
                resolved.available_branches.join(", ")
            )
        } else {
            format!("no indexed store found at {}", gitnexus_dir.display())
        };
        warnings.extend(gitnexus_warnings(
            gitnexus_dir_override,
            project_root,
            Some(&gitnexus_dir),
            false,
            Some(&load_error),
        ));
        return ComposableResolve {
            mdg: None,
            warnings,
        };
    };

    if !lbug_path.exists() {
        let load_error = format!("no indexed store found at {}", gitnexus_dir.display());
        warnings.extend(gitnexus_warnings(
            gitnexus_dir_override,
            project_root,
            Some(&gitnexus_dir),
            false,
            Some(&load_error),
        ));
        return ComposableResolve {
            mdg: None,
            warnings,
        };
    }

    match ModuleDependencyGraph::from_lbug_path(&lbug_path, project_root.to_string_lossy()) {
        Ok(graph) => {
            // Freshness is advisory even when the graph loads — surface it the
            // same way MCP does so CLI/MCP do not disagree on stale stores.
            warnings.extend(gitnexus_warnings(
                gitnexus_dir_override,
                project_root,
                Some(&gitnexus_dir),
                true,
                None,
            ));
            ComposableResolve {
                mdg: Some(graph),
                warnings,
            }
        }
        Err(e) => {
            let load_error = e.to_string();
            warnings.extend(gitnexus_warnings(
                gitnexus_dir_override,
                project_root,
                Some(&gitnexus_dir),
                false,
                Some(&load_error),
            ));
            ComposableResolve {
                mdg: None,
                warnings,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topos_mcp::evaluation::{
        depgraph_status, gitnexus_warnings, resolve_composable_project_root,
        resolve_override_for_root, INVALID_GITNEXUS_MARKERS,
    };

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_cli_composable_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_composable_mdg_returns_none_for_override_outside_project_root_without_shelling_out()
    {
        // An override outside `project_root` is rejected by
        // `resolve_gitnexus_dir`/`depgraph_status` before any
        // availability check or subprocess call, so this stays
        // deterministic regardless of whether gitnexus happens to be
        // installed on the machine running the test.
        let project_root = temp_dir("root");
        let outside = temp_dir("outside");

        let result = resolve_composable_mdg(
            &project_root,
            Some(&outside.to_string_lossy()),
            false,
            &mut |_| {},
        );
        assert!(result.mdg.is_none());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains(INVALID_GITNEXUS_MARKERS[0])),
            "outside-root override must surface the shared invalid-dir warning, got {:?}",
            result.warnings
        );

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn in_root_missing_override_is_missing_not_invalid() {
        // #287: naming a not-yet-created store inside the project root must
        // classify as `missing` so ensure_gitnexus_dir will generate, not as
        // `invalid_dir` (which skips generation entirely).
        let home = temp_dir("missing_home");
        let repo = home.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = repo.join(".gitnexus");
        assert!(!store.exists());

        let project_root = resolve_composable_project_root(Some(store.to_str().unwrap()), &home);
        // macOS temp dirs often live under /var → /private/var; compare
        // canonical forms so the assertion is about the store parent, not
        // the symlink spelling of $TMPDIR.
        let repo_canon = repo.canonicalize().unwrap();
        assert_eq!(project_root, repo_canon);

        let resolved = resolve_override_for_root(Some(store.to_str().unwrap()), &home).unwrap();
        let status = depgraph_status(
            Some(&resolved),
            &project_root,
            &repo_canon.to_string_lossy(),
        );
        assert_eq!(
            status.state, "missing",
            "first-run in-root override must be missing so generation can run"
        );
        // Until ensure generates, warnings explain missing — never invalid_dir markers.
        let warns = gitnexus_warnings(Some(&resolved), &project_root, None, false, None);
        assert!(
            warns
                .iter()
                .all(|w| !INVALID_GITNEXUS_MARKERS.iter().any(|m| w.contains(m))),
            "in-root missing override must not emit invalid_gitnexus markers: {warns:?}"
        );
        assert!(
            warns
                .iter()
                .any(|w| w.contains("no .gitnexus directory found")),
            "expected missing-store guidance, got {warns:?}"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// `inspect` always captures GitNexus output: JSON must remain parseable,
    /// while human output owns its transient spinner and stable stderr text.
    #[test]
    fn inspect_always_captures_gitnexus_output() {
        // Collapse whitespace so rustfmt line-wrapping can't break the match.
        let src: String = include_str!("inspect/mod.rs")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            src.contains("resolve_composable_mdg(")
                && src.contains("resolved_override.as_deref(), true,")
                && src.contains("&mut on_phase"),
            "inspect must capture GitNexus output so it cannot corrupt the CLI renderer"
        );
    }
}
