//! Native LadybugDB loader via the `lbug` Rust crate (issue #198).
//!
//! Primary path for GitNexus ≥ 1.5 binary stores: open `.gitnexus/lbug`
//! in-process, query File nodes + `CodeRelation` edges, stub other
//! endpoints.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use lbug::{Connection, Database, SystemConfig, Value as LbugValue};
use serde_json::Value;

use super::models::{GraphNode, GraphRelationship};
use super::object::{MdgError, ModuleDependencyGraph};

impl ModuleDependencyGraph {
    /// Load a binary LadybugDB store with the embedded `lbug` client.
    pub fn from_ladybug_native(
        lbug_path: &Path,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let mut graph = ModuleDependencyGraph::new(target_file);
        let db = open_database(lbug_path)?;
        let conn =
            Connection::new(&db).map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;
        load_file_nodes(&mut graph, &conn)?;
        load_relationships(&mut graph, &conn)?;
        stub_missing_endpoints(&mut graph);
        Ok(graph)
    }
}

fn open_database(lbug_path: &Path) -> Result<Database, MdgError> {
    match Database::new(lbug_path, SystemConfig::default().read_only(true)) {
        Ok(db) => Ok(db),
        Err(read_only_err) => {
            // Pending shadow pages (incremental `gitnexus analyze`) may
            // require a read-write open to replay the WAL — same retry as
            // the old Python `_from_ladybugdb` path.
            Database::new(lbug_path, SystemConfig::default()).map_err(|rw_err| {
                MdgError::LadybugNativeFailed(format!(
                    "read_only open failed ({read_only_err}); read_write retry failed ({rw_err})"
                ))
            })
        }
    }
}

fn as_string(value: &LbugValue) -> String {
    match value {
        LbugValue::String(s) => s.clone(),
        LbugValue::Null(_) => String::new(),
        other => other.to_string(),
    }
}

fn load_file_nodes(
    graph: &mut ModuleDependencyGraph,
    conn: &Connection<'_>,
) -> Result<(), MdgError> {
    let result = conn
        .query("MATCH (n:File) RETURN n.id, n.filePath, n.name")
        .map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;
    for row in result {
        if row.is_empty() {
            continue;
        }
        let id = as_string(&row[0]);
        if id.is_empty() {
            continue;
        }
        let mut properties = HashMap::new();
        if row.len() > 1 {
            let path = as_string(&row[1]);
            if !path.is_empty() {
                properties.insert("filePath".to_string(), Value::String(path));
            }
        }
        if row.len() > 2 {
            let name = as_string(&row[2]);
            if !name.is_empty() {
                properties.insert("name".to_string(), Value::String(name));
            }
        }
        graph.add_node(GraphNode {
            id,
            label: "File".to_string(),
            properties,
        });
    }
    Ok(())
}

fn load_relationships(
    graph: &mut ModuleDependencyGraph,
    conn: &Connection<'_>,
) -> Result<(), MdgError> {
    let with_step = conn
        .query(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id, dst.id, r.type, r.step LIMIT 1",
        )
        .is_ok();
    let result = if with_step {
        conn.query(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id, dst.id, r.type, r.step",
        )
    } else {
        conn.query(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id, dst.id, r.type",
        )
    }
    .map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;

    for (idx, row) in result.enumerate() {
        if row.len() < 3 {
            continue;
        }
        let source_id = as_string(&row[0]);
        let target_id = as_string(&row[1]);
        let rel_type = as_string(&row[2]);
        if source_id.is_empty() || target_id.is_empty() || rel_type.is_empty() {
            continue;
        }
        let mut properties = HashMap::new();
        if with_step && row.len() > 3 {
            match &row[3] {
                LbugValue::Int64(n) => {
                    properties.insert("step".to_string(), Value::Number((*n).into()));
                }
                LbugValue::Int32(n) => {
                    properties.insert("step".to_string(), Value::Number((*n).into()));
                }
                LbugValue::Null(_) => {}
                other => {
                    let s = other.to_string();
                    if !s.is_empty() {
                        properties.insert("step".to_string(), Value::String(s));
                    }
                }
            }
        }
        graph.add_relationship(GraphRelationship {
            id: format!("{source_id}->{target_id}:{rel_type}:{idx}"),
            source_id,
            target_id,
            rel_type,
            confidence: 1.0,
            reason: String::new(),
            properties,
        });
    }
    Ok(())
}

/// COMPOSABLE walks CONTAINS/CALLS/IMPORTS by id; non-File endpoints only
/// need to exist in `nodes` so lookups succeed.
pub(crate) fn stub_missing_endpoints(graph: &mut ModuleDependencyGraph) {
    let mut needed: HashSet<String> = HashSet::new();
    for rel in graph.relationships.values() {
        needed.insert(rel.source_id.clone());
        needed.insert(rel.target_id.clone());
    }
    for id in needed {
        if graph.nodes.contains_key(&id) {
            continue;
        }
        graph.add_node(GraphNode {
            id,
            label: "Symbol".to_string(),
            properties: HashMap::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_missing_endpoints_adds_symbol_nodes() {
        let mut graph = ModuleDependencyGraph::new("a.rs");
        graph.add_node(GraphNode {
            id: "File:a.rs".into(),
            label: "File".into(),
            properties: HashMap::from([("filePath".into(), Value::String("a.rs".into()))]),
        });
        graph.add_relationship(GraphRelationship {
            id: "r1".into(),
            source_id: "File:a.rs".into(),
            target_id: "Function:a.rs:foo".into(),
            rel_type: "CONTAINS".into(),
            confidence: 1.0,
            reason: String::new(),
            properties: HashMap::new(),
        });
        stub_missing_endpoints(&mut graph);
        assert!(graph.nodes.contains_key("Function:a.rs:foo"));
        assert_eq!(
            graph.contained_symbols("File:a.rs"),
            vec!["Function:a.rs:foo".to_string()]
        );
    }
}
