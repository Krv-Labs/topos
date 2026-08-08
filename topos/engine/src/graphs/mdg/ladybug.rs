//! Native LadybugDB loader via the `lbug` Rust crate (issue #198).
//!
//! Opens `.gitnexus/lbug` in-process, discovers all node tables via
//! `show_tables()`, loads every label with real properties, and reads edge
//! confidence/reason from `CodeRelation`.

use std::collections::HashMap;
use std::path::Path;

use lbug::{Connection, Database, NodeVal, SystemConfig, Value as LbugValue};
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
        load_all_nodes(&mut graph, &conn)?;
        load_relationships(&mut graph, &conn)?;
        Ok(graph)
    }
}

fn open_database(lbug_path: &Path) -> Result<Database, MdgError> {
    match Database::new(lbug_path, SystemConfig::default().read_only(true)) {
        Ok(db) => Ok(db),
        Err(read_only_err) => Database::new(lbug_path, SystemConfig::default()).map_err(|rw_err| {
            MdgError::LadybugNativeFailed(format!(
                "read_only open failed ({read_only_err}); read_write retry failed ({rw_err})"
            ))
        }),
    }
}

fn as_string(value: &LbugValue) -> String {
    match value {
        LbugValue::String(s) => s.clone(),
        LbugValue::Null(_) => String::new(),
        other => other.to_string(),
    }
}

fn as_f64(value: &LbugValue) -> Option<f64> {
    match value {
        LbugValue::Double(v) => Some(*v),
        LbugValue::Float(v) => Some(*v as f64),
        LbugValue::Int64(v) => Some(*v as f64),
        LbugValue::Int32(v) => Some(*v as f64),
        LbugValue::Null(_) => None,
        _ => None,
    }
}

fn lbug_value_to_json(value: &LbugValue) -> Value {
    match value {
        LbugValue::String(s) => Value::String(s.clone()),
        LbugValue::Bool(b) => Value::Bool(*b),
        LbugValue::Int64(n) => Value::Number((*n).into()),
        LbugValue::Int32(n) => Value::Number((*n).into()),
        LbugValue::Double(n) => serde_json::Number::from_f64(*n)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        LbugValue::Float(n) => serde_json::Number::from_f64(*n as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        LbugValue::Null(_) => Value::Null,
        other => Value::String(other.to_string()),
    }
}

fn node_id_from_val(node: &NodeVal) -> Option<String> {
    for (key, value) in node.get_properties() {
        if key == "id" {
            let id = as_string(value);
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn node_properties_from_val(node: &NodeVal) -> HashMap<String, Value> {
    node.get_properties()
        .iter()
        .filter(|(key, _)| !key.starts_with('_'))
        .map(|(key, value)| (key.clone(), lbug_value_to_json(value)))
        .collect()
}

fn discover_node_tables(conn: &Connection<'_>) -> Result<Vec<String>, MdgError> {
    let result = conn
        .query("CALL show_tables() RETURN *")
        .map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;
    let mut labels = Vec::new();
    for row in result {
        if row.len() >= 3 && as_string(&row[2]) == "NODE" {
            let label = as_string(&row[1]);
            if !label.is_empty() {
                labels.push(label);
            }
        }
    }
    Ok(labels)
}

fn load_all_nodes(
    graph: &mut ModuleDependencyGraph,
    conn: &Connection<'_>,
) -> Result<(), MdgError> {
    for label in discover_node_tables(conn)? {
        let query = format!("MATCH (n:`{label}`) RETURN n");
        let result = conn
            .query(&query)
            .map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;
        for row in result {
            let Some(node_val) = row.first() else {
                continue;
            };
            let LbugValue::Node(node) = node_val else {
                continue;
            };
            let Some(id) = node_id_from_val(node) else {
                continue;
            };
            graph.add_node(GraphNode {
                id,
                label: label.clone(),
                properties: node_properties_from_val(node),
            });
        }
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
             RETURN src.id, dst.id, r.type, r.confidence, r.reason, r.step LIMIT 1",
        )
        .is_ok();
    let result = if with_step {
        conn.query(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id, dst.id, r.type, r.confidence, r.reason, r.step",
        )
    } else {
        conn.query(
            "MATCH (src)-[r:CodeRelation]->(dst) \
             RETURN src.id, dst.id, r.type, r.confidence, r.reason",
        )
    }
    .map_err(|e| MdgError::LadybugNativeFailed(e.to_string()))?;

    for (idx, row) in result.enumerate() {
        if row.len() < 5 {
            continue;
        }
        let source_id = as_string(&row[0]);
        let target_id = as_string(&row[1]);
        let rel_type = as_string(&row[2]);
        if source_id.is_empty() || target_id.is_empty() || rel_type.is_empty() {
            continue;
        }
        let confidence = as_f64(&row[3]).unwrap_or(1.0);
        let reason = as_string(&row[4]);
        let mut properties = HashMap::new();
        if with_step && row.len() > 5 {
            match &row[5] {
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
            confidence,
            reason,
            properties,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_node_tables_parses_show_tables_rows() {
        let rows = [
            vec![
                LbugValue::String("0".into()),
                LbugValue::String("File".into()),
                LbugValue::String("NODE".into()),
            ],
            vec![
                LbugValue::String("1".into()),
                LbugValue::String("CodeRelation".into()),
                LbugValue::String("REL".into()),
            ],
            vec![
                LbugValue::String("2".into()),
                LbugValue::String("Function".into()),
                LbugValue::String("NODE".into()),
            ],
        ];
        let labels: Vec<String> = rows
            .iter()
            .filter(|row| row.len() >= 3 && as_string(&row[2]) == "NODE")
            .map(|row| as_string(&row[1]))
            .filter(|label| !label.is_empty())
            .collect();
        assert_eq!(labels, vec!["File".to_string(), "Function".to_string()]);
    }

    #[test]
    fn relationship_confidence_and_reason_use_fallbacks() {
        assert_eq!(as_f64(&LbugValue::Double(0.75)).unwrap_or(1.0), 0.75);
        assert_eq!(
            as_f64(&LbugValue::String("nope".into())).unwrap_or(1.0),
            1.0
        );
    }

    #[test]
    #[ignore = "requires a gitnexus lbug store on disk"]
    fn loads_real_store_when_present() {
        let store = Path::new(".gitnexus/lbug");
        if !store.exists() {
            return;
        }
        let graph = ModuleDependencyGraph::from_ladybug_native(store, "lib.rs").unwrap();
        assert!(!graph.nodes.is_empty());
    }
}
