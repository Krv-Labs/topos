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

thread_local! {
    /// Wall time of the most recent store open that actually read Ladybug.
    ///
    /// A cache hit records `0`. Tests read this; it is not an agent-facing
    /// field. Per thread, so parallel tests cannot overwrite each other's value.
    static LAST_LOAD_MS: std::cell::Cell<u128> = const { std::cell::Cell::new(0) };
}

/// Clear the dep-graph cache (primarily for tests).
pub fn clear_caches() {
    if let Ok(mut guard) = STORE_CACHE.lock() {
        *guard = None;
    }
    LAST_LOAD_MS.with(|ms| ms.set(0));
}

/// Milliseconds spent inside the last Ladybug open. `0` when the last
/// [`load_dep_graph`] was served from the process store.
pub fn last_store_load_ms() -> u128 {
    LAST_LOAD_MS.with(std::cell::Cell::get)
}

fn remember_load_ms(started: Instant, hit: bool) {
    let ms = if hit {
        0
    } else {
        started.elapsed().as_millis()
    };
    LAST_LOAD_MS.with(|cell| cell.set(ms));
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
    let branch = gitnexus_dir
        .canonicalize()
        .unwrap_or_else(|_| gitnexus_dir.to_path_buf())
        .parent()
        .and_then(current_git_branch);
    match store_graph(gitnexus_dir, branch, target_file) {
        Ok(graph) => (Some(graph), None),
        Err(err) => (None, Some(err)),
    }
}

/// The process store retargeted at `target_file`, opening Ladybug only
/// when `(dir, branch, mtime)` differs from the cached store.
fn store_graph(
    gitnexus_dir: &Path,
    branch: Option<String>,
    target_file: &str,
) -> Result<ModuleDependencyGraph, String> {
    let started = Instant::now();
    let gitnexus_dir = gitnexus_dir
        .canonicalize()
        .unwrap_or_else(|_| gitnexus_dir.to_path_buf());
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
                return Ok(graph);
            }
        }
    }

    let loaded = load_mdg_branch_aware(&gitnexus_dir, target_file, branch.as_deref());
    if let (Ok(graph), Ok(mut guard)) = (&loaded, STORE_CACHE.lock()) {
        *guard = Some(StoreCache {
            dir,
            branch,
            mtime_bits,
            graph: graph.clone(),
        });
    }
    remember_load_ms(started, false);
    loaded
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
    // Through the process store: the first status pays the open that the
    // following `load_dep_graph` then reuses, and a half-written or
    // wrong-schema store still reports `load_error` / `schema_mismatch`.
    if let Err(msg) = store_graph(&gitnexus_dir, branch, target_file) {
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

    /// Both tests load the real store and clear the process cache; the
    /// lock keeps one from clearing it between the other's two calls.
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
    fn status_reopens_neither_a_loaded_store_nor_a_second_file() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(dir) = repo_gitnexus() else {
            return;
        };
        clear_caches();
        crate::evaluation::clear_freshness_cache();
        let root = dir.parent().unwrap();
        let _ = depgraph_status(None, root, "topos/mcp/src/lib.rs");
        assert!(last_store_load_ms() > 0, "first status must open the store");
        let status = depgraph_status(None, root, "topos/mcp/src/server.rs");
        assert!(
            matches!(status.state, "present" | "stale"),
            "unexpected status {} ({})",
            status.state,
            status.detail.unwrap_or_default()
        );
        assert_eq!(last_store_load_ms(), 0, "second status reopened Ladybug");
        let (graph, err) = load_dep_graph(
            Some(&dir),
            &dir.join("../topos/mcp/src/lib.rs").to_string_lossy(),
        );
        assert!(err.is_none() && graph.is_some(), "{err:?}");
        assert_eq!(
            last_store_load_ms(),
            0,
            "load after status reopened Ladybug"
        );
        clear_caches();
    }
}

#[cfg(test)]
mod broken_store_tests {
    use super::*;

    #[test]
    fn half_written_store_is_a_load_error_not_present() {
        let root = std::env::temp_dir().join(format!("topos_broken_store_{}", std::process::id()));
        let lbug = root.join(".gitnexus/lbug");
        std::fs::create_dir_all(root.join(".gitnexus")).unwrap();
        std::fs::write(&lbug, b"not a ladybug store").unwrap();
        let status = depgraph_status(None, &root, &root.to_string_lossy());
        std::fs::remove_dir_all(&root).ok();
        assert_ne!(status.state, "present", "{:?}", status.detail);
        assert_ne!(status.state, "stale", "{:?}", status.detail);
    }
}
