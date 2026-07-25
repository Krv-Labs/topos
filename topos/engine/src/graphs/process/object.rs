//! Process graph representation.
//!
//! Consumes the `Process` nodes and `STEP_IN_PROCESS` relationships already
//! present in the GitNexus knowledge graph (see [`crate::graphs::mdg`]) and
//! lifts them into ordered execution paths.
//!
//! This is a **refactoring-tool** input, not a scored
//! [`crate::graphs::base::Representation`]: process-graph analysis
//! (issue #86) must never influence the SIMPLE, COMPOSABLE, or SECURE medal
//! computation. It exists purely to feed the directed Forman-Ricci curvature
//! engine ([`crate::functors::probes::process::curvature`]) that powers
//! `topos refactor process`.
//!
//! Reuses [`ModuleDependencyGraph`]'s existing loading machinery (including
//! schema-mismatch handling) rather than opening a second connection to
//! `.gitnexus/lbug` — a [`ProcessGraph`] is built by loading a full MDG,
//! then filtering it down to `Process` / `STEP_IN_PROCESS` structure.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use crate::graphs::mdg::object::{MdgError, ModuleDependencyGraph};

/// One step (code element) participating in a process execution path.
#[derive(Debug, Clone)]
pub struct ProcessStep {
    pub node_id: String,
    pub label: String,
    pub step: i64,
    pub properties: HashMap<String, Value>,
}

/// An ordered execution path through a single `Process` node.
#[derive(Debug, Clone, Default)]
pub struct ProcessPath {
    pub process_id: String,
    pub steps: Vec<ProcessStep>,
}

/// Ordered execution-path view of GitNexus `Process` / `STEP_IN_PROCESS`
/// data, scoped to a target file for the `topos refactor process` tool.
#[derive(Debug, Clone, Default)]
pub struct ProcessGraph {
    /// The file path this graph was built to analyze.
    pub target_file: String,
    /// Every `Process` node's execution path, steps sorted ascending.
    pub paths: Vec<ProcessPath>,
    /// The MDG this graph was filtered from; needed by
    /// [`ProcessGraph::paths_touching_file`]'s containment walk. `None` for
    /// hand-built graphs (e.g. a subgraph of an existing one).
    mdg: Option<ModuleDependencyGraph>,
}

impl ProcessGraph {
    /// A bare graph with no backing MDG (used for filtered subgraphs).
    pub fn from_paths(target_file: impl Into<String>, paths: Vec<ProcessPath>) -> Self {
        ProcessGraph {
            target_file: target_file.into(),
            paths,
            mdg: None,
        }
    }

    /// Filter an already-loaded MDG down to Process/STEP_IN_PROCESS structure.
    pub fn from_mdg(mdg: &ModuleDependencyGraph, target_file: impl Into<String>) -> Self {
        let process_ids: HashSet<&str> = mdg
            .nodes_of_label("Process")
            .into_iter()
            .map(|n| n.id.as_str())
            .collect();

        // Group STEP_IN_PROCESS relationships by their owning process. The
        // Python original leaned on dict insertion order for the fallback
        // step index; Rust's HashMap iteration is unordered, so sort by
        // relationship id up front to keep the fallback deterministic.
        let mut step_rels: Vec<&crate::graphs::mdg::models::GraphRelationship> = mdg
            .relationships_of_type("STEP_IN_PROCESS")
            .into_iter()
            .filter(|rel| process_ids.contains(rel.source_id.as_str()))
            .collect();
        step_rels.sort_by(|a, b| a.id.cmp(&b.id));

        let mut order: Vec<&str> = Vec::new();
        let mut rels_by_process: HashMap<
            &str,
            Vec<&crate::graphs::mdg::models::GraphRelationship>,
        > = HashMap::new();
        for rel in step_rels {
            let entry = rels_by_process.entry(rel.source_id.as_str()).or_default();
            if entry.is_empty() {
                order.push(rel.source_id.as_str());
            }
            entry.push(rel);
        }

        let mut paths = Vec::with_capacity(order.len());
        for process_id in order {
            let rels = &rels_by_process[process_id];
            let mut steps: Vec<ProcessStep> = rels
                .iter()
                .enumerate()
                .map(|(fallback_order, rel)| {
                    let label = mdg
                        .get_node(&rel.target_id)
                        .map(|n| n.label.clone())
                        .unwrap_or_default();
                    let step = step_index(rel.properties.get("step"), fallback_order as i64);
                    ProcessStep {
                        node_id: rel.target_id.clone(),
                        label,
                        step,
                        properties: rel.properties.clone(),
                    }
                })
                .collect();
            steps.sort_by_key(|s| s.step);
            paths.push(ProcessPath {
                process_id: process_id.to_string(),
                steps,
            });
        }

        ProcessGraph {
            target_file: target_file.into(),
            paths,
            mdg: Some(mdg.clone()),
        }
    }

    /// Load a full MDG from `.gitnexus/` and filter it to process structure.
    pub fn from_gitnexus_dir(
        gitnexus_dir: impl AsRef<Path>,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let target_file = target_file.into();
        let mdg = ModuleDependencyGraph::from_gitnexus_dir(gitnexus_dir, target_file.clone())?;
        Ok(Self::from_mdg(&mdg, target_file))
    }

    /// Paths where any step's underlying symbol is contained in `file_node_id`.
    pub fn paths_touching_file(&self, file_node_id: &str) -> Vec<ProcessPath> {
        let Some(mdg) = &self.mdg else {
            return Vec::new();
        };
        let mut symbol_ids: HashSet<String> = mdg
            .all_contained_symbols(file_node_id)
            .into_iter()
            .collect();
        symbol_ids.insert(file_node_id.to_string());
        self.paths
            .iter()
            .filter(|path| path.steps.iter().any(|s| symbol_ids.contains(&s.node_id)))
            .cloned()
            .collect()
    }

    /// Flatten every path's consecutive step pairs into directed edges —
    /// the input to [`crate::functors::curvature::directed_forman_curvature`].
    pub fn edges(&self) -> Vec<(&str, &str)> {
        let mut result = Vec::new();
        for path in &self.paths {
            for pair in path.steps.windows(2) {
                result.push((pair[0].node_id.as_str(), pair[1].node_id.as_str()));
            }
        }
        result
    }
}

fn step_index(step_value: Option<&Value>, fallback_order: i64) -> i64 {
    match step_value {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(fallback_order),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(fallback_order),
        _ => fallback_order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};

    fn node(id: &str, label: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties: HashMap::new(),
        }
    }

    fn step_rel(process: &str, target: &str, step: Option<i64>) -> GraphRelationship {
        let mut properties = HashMap::new();
        if let Some(s) = step {
            properties.insert("step".to_string(), Value::from(s));
        }
        GraphRelationship {
            id: format!("{process}->{target}"),
            source_id: process.to_string(),
            target_id: target.to_string(),
            rel_type: "STEP_IN_PROCESS".to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties,
        }
    }

    fn sample_mdg() -> ModuleDependencyGraph {
        let mut mdg = ModuleDependencyGraph::new("main.py");
        mdg.add_node(node("proc:checkout", "Process"));
        mdg.add_node(node("fn:a", "Function"));
        mdg.add_node(node("fn:b", "Function"));
        mdg.add_node(node("fn:c", "Function"));
        // Deliberately out of order: step ordinals must drive the sort.
        mdg.add_relationship(step_rel("proc:checkout", "fn:c", Some(2)));
        mdg.add_relationship(step_rel("proc:checkout", "fn:a", Some(0)));
        mdg.add_relationship(step_rel("proc:checkout", "fn:b", Some(1)));
        mdg
    }

    #[test]
    fn from_mdg_orders_steps_by_step_property() {
        let graph = ProcessGraph::from_mdg(&sample_mdg(), "main.py");
        assert_eq!(graph.paths.len(), 1);
        let ids: Vec<&str> = graph.paths[0]
            .steps
            .iter()
            .map(|s| s.node_id.as_str())
            .collect();
        assert_eq!(ids, vec!["fn:a", "fn:b", "fn:c"]);
    }

    #[test]
    fn edges_flatten_consecutive_steps() {
        let graph = ProcessGraph::from_mdg(&sample_mdg(), "main.py");
        assert_eq!(graph.edges(), vec![("fn:a", "fn:b"), ("fn:b", "fn:c")]);
    }

    #[test]
    fn missing_step_property_falls_back_to_encounter_order() {
        let mut mdg = ModuleDependencyGraph::new("main.py");
        mdg.add_node(node("proc:p", "Process"));
        mdg.add_node(node("fn:x", "Function"));
        mdg.add_node(node("fn:y", "Function"));
        mdg.add_relationship(step_rel("proc:p", "fn:x", None));
        mdg.add_relationship(step_rel("proc:p", "fn:y", None));
        let graph = ProcessGraph::from_mdg(&mdg, "main.py");
        let ids: Vec<&str> = graph.paths[0]
            .steps
            .iter()
            .map(|s| s.node_id.as_str())
            .collect();
        assert_eq!(ids, vec!["fn:x", "fn:y"]);
    }

    #[test]
    fn non_process_sources_are_ignored() {
        let mut mdg = sample_mdg();
        mdg.add_node(node("fn:rogue", "Function"));
        mdg.add_relationship(step_rel("fn:rogue", "fn:a", Some(0)));
        let graph = ProcessGraph::from_mdg(&mdg, "main.py");
        assert_eq!(graph.paths.len(), 1);
        assert_eq!(graph.paths[0].process_id, "proc:checkout");
    }

    #[test]
    fn paths_touching_file_without_mdg_is_empty() {
        let graph = ProcessGraph::from_paths("main.py", vec![ProcessPath::default()]);
        assert!(graph.paths_touching_file("file:main").is_empty());
    }
}
