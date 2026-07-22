//! Fallback COMPOSABLE loader: shell out to `gitnexus cypher` (issue #198).
//!
//! Primary path is the embedded [`super::ladybug`] client. This module is
//! used when native open/query fails (version skew, missing native build,
//! etc.).
//!
//! # Performance constraints
//!
//! - GitNexus truncates cypher stdout at 64 KiB → every bulk `MATCH` is
//!   paged with `SKIP`/`LIMIT`.
//! - Each `gitnexus cypher` spawn costs ~0.7s, so we avoid per-label
//!   discovery: load **File** nodes plus all `CodeRelation` edges, then
//!   stub other endpoints. Relationship pages are fetched concurrently.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::adapters::gitnexus::gitnexus_available;
use crate::adapters::gitnexus_cypher::{run_cypher, CypherError, CypherTable};

use super::ladybug::stub_missing_endpoints;
use super::models::{GraphNode, GraphRelationship};
use super::object::{MdgError, ModuleDependencyGraph};

const FILE_PAGE_SIZE: usize = 150;
const REL_PAGE_SIZE: usize = 250;
const REL_PARALLELISM: usize = 8;

impl ModuleDependencyGraph {
    /// Load from whichever on-disk shape `lbug_path` has.
    ///
    /// - Directory → legacy JSON export (GitNexus < 1.5)
    /// - File → native `lbug`, then `gitnexus cypher` fallback
    pub fn from_lbug_path(
        lbug_path: &Path,
        target_file: impl Into<String>,
        project_root: &Path,
        branch: Option<&str>,
    ) -> Result<Self, MdgError> {
        if lbug_path.is_dir() {
            return Self::from_json_dir(lbug_path, target_file);
        }
        if !lbug_path.is_file() {
            return Err(MdgError::NotFound(lbug_path.to_path_buf()));
        }

        let target_file = target_file.into();
        match Self::from_ladybug_native(lbug_path, target_file.clone()) {
            Ok(graph) => Ok(graph),
            Err(native_err) => {
                if !gitnexus_available() {
                    return Err(MdgError::LadybugNativeFailed(format!(
                        "{native_err}; cypher fallback unavailable (gitnexus not on PATH)"
                    )));
                }
                match Self::from_cypher(project_root, branch, target_file) {
                    Ok(graph) => Ok(graph),
                    Err(cypher_err) => Err(MdgError::CypherFailed(format!(
                        "native lbug failed ({native_err}); cypher fallback failed ({cypher_err})"
                    ))),
                }
            }
        }
    }

    /// Build a graph by querying the Ladybug binary store via `gitnexus cypher`.
    pub fn from_cypher(
        project_root: &Path,
        branch: Option<&str>,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let mut graph = ModuleDependencyGraph::new(target_file);
        load_file_nodes(&mut graph, project_root, branch)?;
        load_relationships(&mut graph, project_root, branch)?;
        stub_missing_endpoints(&mut graph);
        Ok(graph)
    }
}

fn cypher(project_root: &Path, branch: Option<&str>, query: &str) -> Result<CypherTable, MdgError> {
    run_cypher(project_root, branch, query).map_err(cypher_err)
}

fn cypher_err(err: CypherError) -> MdgError {
    match err {
        CypherError::NotAvailable => MdgError::CypherUnavailable(Path::new("lbug").to_path_buf()),
        other => MdgError::CypherFailed(other.to_string()),
    }
}

fn load_file_nodes(
    graph: &mut ModuleDependencyGraph,
    project_root: &Path,
    branch: Option<&str>,
) -> Result<(), MdgError> {
    let mut skip = 0usize;
    loop {
        let query = format!(
            "MATCH (n:`File`) RETURN n.id AS id, n.filePath AS filePath, n.name AS name \
             ORDER BY n.id SKIP {skip} LIMIT {FILE_PAGE_SIZE}"
        );
        let table = cypher(project_root, branch, &query)?;
        if table.rows.is_empty() {
            break;
        }
        for row in &table.rows {
            let Some(id) = table.get(row, "id") else {
                continue;
            };
            let mut properties = HashMap::new();
            if let Some(path) = table.get(row, "filePath") {
                properties.insert("filePath".to_string(), Value::String(path.to_string()));
            }
            if let Some(name) = table.get(row, "name") {
                properties.insert("name".to_string(), Value::String(name.to_string()));
            }
            graph.add_node(GraphNode {
                id: id.to_string(),
                label: "File".to_string(),
                properties,
            });
        }
        let n = table.rows.len();
        skip += n;
        if n < FILE_PAGE_SIZE {
            break;
        }
    }
    Ok(())
}

fn count_relationships(project_root: &Path, branch: Option<&str>) -> Result<usize, MdgError> {
    let table = cypher(
        project_root,
        branch,
        "MATCH (src)-[r:CodeRelation]->(dst) RETURN count(r) AS c",
    )?;
    let raw = table
        .rows
        .first()
        .and_then(|row| table.get(row, "c"))
        .unwrap_or("0");
    raw.parse::<usize>()
        .map_err(|e| MdgError::CypherFailed(format!("bad relationship count '{raw}': {e}")))
}

fn rel_query(skip: usize, with_step: bool) -> String {
    if with_step {
        format!(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id AS sourceId, dst.id AS targetId, r.type AS type, r.step AS step \
             ORDER BY sourceId, targetId, type SKIP {skip} LIMIT {REL_PAGE_SIZE}"
        )
    } else {
        format!(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id AS sourceId, dst.id AS targetId, r.type AS type \
             ORDER BY sourceId, targetId, type SKIP {skip} LIMIT {REL_PAGE_SIZE}"
        )
    }
}

fn load_relationships(
    graph: &mut ModuleDependencyGraph,
    project_root: &Path,
    branch: Option<&str>,
) -> Result<(), MdgError> {
    let with_step = run_cypher(
        project_root,
        branch,
        "MATCH (src)-[r:CodeRelation]->(dst) \
         RETURN src.id AS sourceId, dst.id AS targetId, r.type AS type, r.step AS step \
         SKIP 0 LIMIT 1",
    )
    .is_ok();

    let total = count_relationships(project_root, branch)?;
    if total == 0 {
        return Ok(());
    }
    let page_count = total.div_ceil(REL_PAGE_SIZE);
    let skips: Vec<usize> = (0..page_count).map(|i| i * REL_PAGE_SIZE).collect();
    let root = project_root.to_path_buf();
    let branch_owned = branch.map(str::to_string);
    let tables = fetch_rel_pages_parallel(&root, branch_owned.as_deref(), &skips, with_step)?;

    let mut idx = 0usize;
    for table in tables {
        for row in &table.rows {
            let Some(source_id) = table.get(row, "sourceId") else {
                continue;
            };
            let Some(target_id) = table.get(row, "targetId") else {
                continue;
            };
            let Some(rel_type) = table.get(row, "type") else {
                continue;
            };
            let mut properties = HashMap::new();
            if with_step {
                if let Some(step) = table.get(row, "step") {
                    if let Ok(n) = step.parse::<i64>() {
                        properties.insert("step".to_string(), Value::Number(n.into()));
                    } else if !step.is_empty() {
                        properties.insert("step".to_string(), Value::String(step.to_string()));
                    }
                }
            }
            graph.add_relationship(GraphRelationship {
                id: format!("{source_id}->{target_id}:{rel_type}:{idx}"),
                source_id: source_id.to_string(),
                target_id: target_id.to_string(),
                rel_type: rel_type.to_string(),
                confidence: 1.0,
                reason: String::new(),
                properties,
            });
            idx += 1;
        }
    }
    Ok(())
}

fn fetch_rel_pages_parallel(
    project_root: &Path,
    branch: Option<&str>,
    skips: &[usize],
    with_step: bool,
) -> Result<Vec<CypherTable>, MdgError> {
    let root = PathBuf::from(project_root);
    let branch_owned = branch.map(str::to_string);
    let mut results: Vec<CypherTable> = Vec::with_capacity(skips.len());

    for chunk in skips.chunks(REL_PARALLELISM) {
        let chunk = chunk.to_vec();
        let collected: Mutex<Vec<(usize, Result<CypherTable, MdgError>)>> =
            Mutex::new(Vec::with_capacity(chunk.len()));
        std::thread::scope(|scope| {
            for (local_i, skip) in chunk.iter().enumerate() {
                let root = &root;
                let branch_owned = &branch_owned;
                let collected = &collected;
                let skip = *skip;
                scope.spawn(move || {
                    let query = rel_query(skip, with_step);
                    let result = cypher(root, branch_owned.as_deref(), &query);
                    if let Ok(mut guard) = collected.lock() {
                        guard.push((local_i, result));
                    }
                });
            }
        });
        let mut page_results = collected
            .into_inner()
            .map_err(|e| MdgError::CypherFailed(e.to_string()))?;
        page_results.sort_by_key(|(i, _)| *i);
        for (_, result) in page_results {
            results.push(result?);
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::gitnexus_cypher::parse_markdown_table;

    #[test]
    fn node_rows_map_to_graph_nodes() {
        let table = parse_markdown_table(
            "| id | filePath | name |\n| --- | --- | --- |\n| File:a.rs | a.rs |  |\n",
        );
        let mut graph = ModuleDependencyGraph::new("a.rs");
        for row in &table.rows {
            let id = table.get(row, "id").unwrap();
            let mut properties = HashMap::new();
            if let Some(path) = table.get(row, "filePath") {
                properties.insert("filePath".to_string(), Value::String(path.to_string()));
            }
            graph.add_node(GraphNode {
                id: id.to_string(),
                label: "File".to_string(),
                properties,
            });
        }
        assert_eq!(graph.file_node_id(), Some("File:a.rs"));
    }
}
