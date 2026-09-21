//! Dep-graph loading, caching, and status reporting.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use topos_engine::adapters::gitnexus::{current_git_branch, resolve_lbug_store};
use topos_engine::graphs::mdg::object::{MdgError, ModuleDependencyGraph};

use super::freshness::graph_freshness;
use super::gitref::{git_head_mtime, gitnexus_mtime};
use super::{
    check_override_warning, is_branch_not_indexed, is_schema_mismatch, resolve_gitnexus_dir,
    BRANCH_NOT_INDEXED_MARKER,
};

/// One loaded Ladybug store for this process.
///
/// The previous cache keyed the whole store by `(dir, target file, mtime)`.
/// A second file was a cache miss, and [`depgraph_status`] reopened the
/// store on every call before the cache was consulted. Both made a warm
/// evaluate pay the cold-load cost again.
struct StoreCache {
    dir: String,
    branch: Option<String>,
    mtime_bits: u64,
    graph: ModuleDependencyGraph,
}

static STORE_CACHE: Mutex<Option<StoreCache>> = Mutex::new(None);

/// Wall time of the most recent store open that actually read Ladybug.
///
/// A cache hit records `0`. Tests and the bench binary read this; it is not
/// an agent-facing field.
static LAST_LOAD_MS: Mutex<u128> = Mutex::new(0);

/// Clear the dep-graph cache (primarily for tests).
pub fn clear_caches() {
    if let Ok(mut guard) = STORE_CACHE.lock() {
        *guard = None;
    }
    if let Ok(mut ms) = LAST_LOAD_MS.lock() {
        *ms = 0;
    }
}

/// Milliseconds spent inside the last Ladybug open. `0` when the last
/// [`load_dep_graph`] was served from the process store.
pub fn last_store_load_ms() -> u128 {
    LAST_LOAD_MS.lock().map(|ms| *ms).unwrap_or(0)
}

fn remember_load_ms(started: Instant, hit: bool) {
    if let Ok(mut ms) = LAST_LOAD_MS.lock() {
        *ms = if hit {
            0
        } else {
            started.elapsed().as_millis()
        };
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

/// Load a `ModuleDependencyGraph` for one file from the process-wide store.
///
/// Returns `(graph, load_error)` — exactly one is `Some`. The store is
/// opened once per `(dir, branch, mtime)`. A later call for another file
/// retargets that store and does not read Ladybug again. A GitNexus re-run
/// or a branch switch changes the key and reloads.
pub fn load_dep_graph(
    gitnexus_dir: Option<&Path>,
    target_file: &str,
) -> (Option<ModuleDependencyGraph>, Option<String>) {
    let Some(gitnexus_dir) = gitnexus_dir else {
        return (None, None);
    };
    let started = Instant::now();
    let gitnexus_dir = gitnexus_dir
        .canonicalize()
        .unwrap_or_else(|_| gitnexus_dir.to_path_buf());
    let branch = gitnexus_dir.parent().and_then(current_git_branch);
    let mtime_bits = gitnexus_mtime(&gitnexus_dir, branch.as_deref())
        .unwrap_or(0.0)
        .to_bits();
    let dir = gitnexus_dir.to_string_lossy().to_string();

    if let Ok(guard) = STORE_CACHE.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.dir == dir && cached.branch == branch && cached.mtime_bits == mtime_bits {
                let graph = cached.graph.for_target(target_file);
                drop(guard);
                remember_load_ms(started, true);
                return (Some(graph), None);
            }
        }
    }

    match load_mdg_branch_aware(&gitnexus_dir, target_file, branch.as_deref()) {
        Ok(graph) => {
            if let Ok(mut guard) = STORE_CACHE.lock() {
                *guard = Some(StoreCache {
                    dir,
                    branch,
                    mtime_bits,
                    graph: graph.clone(),
                });
            }
            remember_load_ms(started, false);
            (Some(graph), None)
        }
        Err(err) => {
            remember_load_ms(started, false);
            (None, Some(err))
        }
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
    // Kept on the signature so status callers that already pass a file do
    // not change. Presence does not depend on which file will be scored.
    let _ = target_file;

    // Presence only. Opening Ladybug here made every evaluate pay the load
    // before `load_dep_graph` could hit its cache. Schema and load failures
    // still surface when that load runs.
    let resolved = resolve_lbug_store(&gitnexus_dir, branch.as_deref());
    if resolved.path.is_none() {
        let msg = if resolved.available_branches.is_empty() {
            MdgError::NotFound(gitnexus_dir.join("lbug")).to_string()
        } else {
            format!(
                "{BRANCH_NOT_INDEXED_MARKER} '{}' (indexed: {})",
                branch.as_deref().unwrap_or("<detached>"),
                resolved.available_branches.join(", ")
            )
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Both tests write `LAST_LOAD_MS`. Parallel cargo test will fail the
    /// zero assertion if the other test records a load in that window.
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn repo_gitnexus() -> Option<std::path::PathBuf> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".gitnexus");
        dir.join("lbug").exists().then_some(dir)
    }

    #[test]
    fn second_file_does_not_reopen_the_store() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(dir) = repo_gitnexus() else {
            return;
        };
        clear_caches();
        let first = dir.join("../topos/mcp/src/lib.rs");
        let second = dir.join("../topos/mcp/src/server.rs");
        let (a, err) = load_dep_graph(Some(&dir), &first.to_string_lossy());
        assert!(err.is_none(), "{err:?}");
        assert!(a.is_some());
        let cold_ms = last_store_load_ms();
        assert!(cold_ms > 0, "cold load recorded no time");

        let (b, err) = load_dep_graph(Some(&dir), &second.to_string_lossy());
        assert!(err.is_none(), "{err:?}");
        let warm = b.expect("warm graph");
        assert_eq!(last_store_load_ms(), 0, "second file reopened Ladybug");
        assert!(
            warm.file_node_id().is_some() || warm.nodes.values().any(|n| n.label == "File"),
            "retargeted graph has no file nodes"
        );
        clear_caches();
    }

    #[test]
    fn status_does_not_open_the_store() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(dir) = repo_gitnexus() else {
            return;
        };
        clear_caches();
        crate::evaluation::clear_freshness_cache();
        let root = dir.parent().unwrap();
        let _ = depgraph_status(None, root, "topos/mcp/src/lib.rs");
        let started = Instant::now();
        let status = depgraph_status(None, root, "topos/mcp/src/server.rs");
        let elapsed = started.elapsed().as_millis();
        assert!(
            matches!(status.state, "present" | "stale"),
            "unexpected status {} ({})",
            status.state,
            status.detail.unwrap_or_default()
        );
        assert!(
            elapsed < 50,
            "second status rewalked the tree: {elapsed} ms for state {}",
            status.state
        );
        assert_eq!(last_store_load_ms(), 0);
    }
}
