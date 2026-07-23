//! Dep-graph loading, caching, and status reporting.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use topos_engine::adapters::gitnexus::{current_git_branch, resolve_lbug_store};
use topos_engine::graphs::mdg::object::{MdgError, ModuleDependencyGraph};

use super::freshness::graph_freshness;
use super::gitref::{git_head_mtime, gitnexus_mtime};
use super::{
    check_override_warning, is_branch_not_indexed, is_schema_mismatch, resolve_gitnexus_dir,
    BRANCH_NOT_INDEXED_MARKER,
};

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
        Some(lbug) => {
            ModuleDependencyGraph::from_lbug_path(&lbug, target_file).map_err(|e| e.to_string())
        }
        None => {
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
