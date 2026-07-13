//! Fan-in / Fan-out metrics — counts incoming and outgoing CALLS edges
//! for a file and its symbols.
//!
//! Fan-in/fan-out is a classic software-engineering measure of module
//! connectivity introduced by Henry & Kafura.
//!
//! - **Fan-in**: how many other symbols call into this file's symbols.
//!   High fan-in means the module is widely depended upon.
//! - **Fan-out**: how many external symbols this file's symbols call.
//!   High fan-out means the module has many dependencies.
//!
//! The product `fan_in * fan_out^2` (the Henry-Kafura complexity) is
//! sometimes used as a structural-risk proxy, but we expose the raw
//! counts so the evaluation section can apply its own thresholds.

use std::collections::HashSet;

use crate::graphs::mdg::object::ModuleDependencyGraph;

/// Fan-in and fan-out counts for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanResult {
    /// Number of distinct external symbols calling into this file.
    pub fan_in: usize,
    /// Number of distinct external symbols called from this file.
    pub fan_out: usize,
}

/// Calculate fan-in and fan-out for a file node.
///
/// Counts distinct external caller/callee symbols connected via `CALLS`
/// relationships to any symbol contained in the file. `symbol_ids`, when
/// supplied, is the pre-computed set of all contained symbol ids
/// (including `file_node_id` itself); it's computed from the graph when
/// `None`.
pub fn calculate_fan_in_out(
    graph: &ModuleDependencyGraph,
    file_node_id: &str,
    symbol_ids: Option<&HashSet<String>>,
) -> FanResult {
    let computed;
    let symbol_ids = match symbol_ids {
        Some(ids) => ids,
        None => {
            let mut ids: HashSet<String> = graph
                .all_contained_symbols(file_node_id)
                .into_iter()
                .collect();
            ids.insert(file_node_id.to_string());
            computed = ids;
            &computed
        }
    };

    let mut external_callers: HashSet<String> = HashSet::new();
    let mut external_callees: HashSet<String> = HashSet::new();

    for sid in symbol_ids {
        for rel in graph.incoming(sid, Some("CALLS")) {
            if !symbol_ids.contains(&rel.source_id) {
                external_callers.insert(rel.source_id.clone());
            }
        }
        for rel in graph.outgoing(sid, Some("CALLS")) {
            if !symbol_ids.contains(&rel.target_id) {
                external_callees.insert(rel.target_id.clone());
            }
        }
    }

    FanResult {
        fan_in: external_callers.len(),
        fan_out: external_callees.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};
    use std::collections::HashMap;

    fn node(id: &str, label: &str, path: Option<&str>) -> GraphNode {
        let mut properties = HashMap::new();
        if let Some(path) = path {
            properties.insert(
                "filePath".to_string(),
                serde_json::Value::String(path.to_string()),
            );
        }
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties,
        }
    }

    fn rel(id: &str, source: &str, target: &str, rel_type: &str) -> GraphRelationship {
        GraphRelationship {
            id: id.to_string(),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 1.0,
            reason: String::new(),
        }
    }

    /// Hub file with many callers and callees.
    fn graph_with_fan() -> ModuleDependencyGraph {
        let mut g = ModuleDependencyGraph::new("hub.py");
        g.add_node(node("File:hub.py", "File", Some("hub.py")));
        g.add_node(node("Func:hub:process", "Function", Some("hub.py")));
        g.add_relationship(rel("c0", "File:hub.py", "Func:hub:process", "CONTAINS"));

        for i in 0..5 {
            let caller_id = format!("Func:caller{i}:run");
            g.add_node(node(&caller_id, "Function", Some(&format!("caller{i}.py"))));
            g.add_relationship(rel(
                &format!("call_in_{i}"),
                &caller_id,
                "Func:hub:process",
                "CALLS",
            ));
        }

        for i in 0..3 {
            let callee_id = format!("Func:dep{i}:work");
            g.add_node(node(&callee_id, "Function", Some(&format!("dep{i}.py"))));
            g.add_relationship(rel(
                &format!("call_out_{i}"),
                "Func:hub:process",
                &callee_id,
                "CALLS",
            ));
        }

        g
    }

    #[test]
    fn fan_in_out() {
        let g = graph_with_fan();
        let result = calculate_fan_in_out(&g, "File:hub.py", None);
        assert_eq!(result.fan_in, 5);
        assert_eq!(result.fan_out, 3);
    }

    #[test]
    fn fan_isolated_file() {
        let mut g = ModuleDependencyGraph::new("solo.py");
        g.add_node(node("File:solo.py", "File", Some("solo.py")));
        let result = calculate_fan_in_out(&g, "File:solo.py", None);
        assert_eq!(result.fan_in, 0);
        assert_eq!(result.fan_out, 0);
    }
}
