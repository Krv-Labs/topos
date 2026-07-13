//! Coupling metrics — quantifies the structural coupling of a module
//! within the dependency graph.
//!
//! Robert C. Martin's package coupling metrics measure how tangled a
//! module is with its neighbours:
//!
//! - **Afferent coupling (Ca)**: number of external modules that depend
//!   on this module (incoming IMPORTS edges).
//! - **Efferent coupling (Ce)**: number of external modules this module
//!   depends on (outgoing IMPORTS edges).
//! - **Instability I = Ce / (Ca + Ce)**: ranges from 0 (maximally
//!   stable, everyone depends on you) to 1 (maximally unstable, you
//!   depend on everyone).
//!
//! High total coupling (Ca + Ce) indicates a module that is hard to
//! change in isolation. Extreme instability or stability *combined*
//! with high coupling is a design smell.
//!
//! Dependency depth measures the longest chain of transitive IMPORTS
//! reachable from the module — deep chains amplify the blast radius of
//! changes.
//!
//! # Deviation from the Python original
//!
//! [`crate::graphs::mdg::object::ModuleDependencyGraph::outgoing`]/
//! [`crate::graphs::mdg::object::ModuleDependencyGraph::incoming`] take
//! `Option<&str>` for the relationship-type filter where Python's take a
//! plain `str` — no behavior difference, just how the pre-existing Rust
//! port (issue #143) shaped the filter.

use std::collections::{HashSet, VecDeque};

use crate::graphs::mdg::object::ModuleDependencyGraph;

/// Coupling metrics for a single module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CouplingResult {
    /// Number of modules that depend on this one (Ca).
    pub afferent: usize,
    /// Number of modules this one depends on (Ce).
    pub efferent: usize,
}

impl CouplingResult {
    pub fn total(&self) -> usize {
        self.afferent + self.efferent
    }
}

/// Calculate afferent and efferent coupling for a file node.
///
/// Counts distinct source/target *File* nodes connected via IMPORTS
/// relationships (directly or through contained symbols).
///
/// `symbol_ids`, when supplied, is the pre-computed set of all contained
/// symbol ids (including `file_node_id` itself); it's computed from the
/// graph when `None`.
pub fn calculate_coupling(
    graph: &ModuleDependencyGraph,
    file_node_id: &str,
    symbol_ids: Option<&HashSet<String>>,
) -> CouplingResult {
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

    let mut efferent_targets: HashSet<String> = HashSet::new();
    let mut afferent_sources: HashSet<String> = HashSet::new();

    for sid in symbol_ids {
        for rel in graph.outgoing(sid, Some("IMPORTS")) {
            if let Some(target_file) = owning_file(graph, &rel.target_id) {
                if target_file != file_node_id {
                    efferent_targets.insert(target_file);
                }
            }
        }
        for rel in graph.incoming(sid, Some("IMPORTS")) {
            if let Some(source_file) = owning_file(graph, &rel.source_id) {
                if source_file != file_node_id {
                    afferent_sources.insert(source_file);
                }
            }
        }
    }

    CouplingResult {
        afferent: afferent_sources.len(),
        efferent: efferent_targets.len(),
    }
}

/// Martin's Instability metric: `I = Ce / (Ca + Ce)`.
///
/// Returns `0.5` when the module has zero coupling (no signal).
pub fn calculate_instability(graph: &ModuleDependencyGraph, file_node_id: &str) -> f64 {
    instability_from_coupling(&calculate_coupling(graph, file_node_id, None))
}

/// Martin's Instability metric from a precomputed coupling result.
///
/// Useful when coupling has already been computed and we want to avoid
/// traversing the graph twice.
pub fn calculate_instability_from_result(result: &CouplingResult) -> f64 {
    instability_from_coupling(result)
}

/// Longest chain of transitive IMPORTS from `file_node_id`.
///
/// Uses BFS to avoid cycles.
pub fn calculate_dependency_depth(graph: &ModuleDependencyGraph, file_node_id: &str) -> usize {
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier: VecDeque<(String, usize)> = VecDeque::from([(file_node_id.to_string(), 0)]);
    let mut max_depth = 0;

    while let Some((current, depth)) = frontier.pop_front() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());
        max_depth = max_depth.max(depth);

        for rel in graph.outgoing(&current, Some("IMPORTS")) {
            if let Some(target_file) = owning_file(graph, &rel.target_id) {
                if !visited.contains(&target_file) {
                    frontier.push_back((target_file, depth + 1));
                }
            }
        }
    }

    max_depth
}

/// Walk up CONTAINS edges to find the File node that owns `node_id`.
pub(crate) fn owning_file(graph: &ModuleDependencyGraph, node_id: &str) -> Option<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut current = node_id.to_string();
    loop {
        if visited.contains(&current) {
            return None; // cycle in CONTAINS chain
        }
        visited.insert(current.clone());
        let node = graph.get_node(&current)?;
        if node.label == "File" {
            return Some(current);
        }
        let parents = graph.incoming(&current, Some("CONTAINS"));
        let parent = parents.first()?;
        current = parent.source_id.clone();
    }
}

fn instability_from_coupling(result: &CouplingResult) -> f64 {
    if result.total() == 0 {
        0.5
    } else {
        result.efferent as f64 / result.total() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};

    fn file_node(id: &str, path: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: "File".to_string(),
            properties: std::collections::HashMap::from([(
                "filePath".to_string(),
                serde_json::Value::String(path.to_string()),
            )]),
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

    /// A -> B -> C -> D linear import chain, target = A.
    fn linear_chain() -> ModuleDependencyGraph {
        let mut g = ModuleDependencyGraph::new("a.py");
        for name in ["a", "b", "c", "d"] {
            g.add_node(file_node(&format!("File:{name}.py"), &format!("{name}.py")));
        }
        g.add_relationship(rel("i1", "File:a.py", "File:b.py", "IMPORTS"));
        g.add_relationship(rel("i2", "File:b.py", "File:c.py", "IMPORTS"));
        g.add_relationship(rel("i3", "File:c.py", "File:d.py", "IMPORTS"));
        g
    }

    #[test]
    fn coupling_linear_chain() {
        let g = linear_chain();
        let file_id = g.file_node_id().unwrap().to_string();
        assert_eq!(file_id, "File:a.py");

        let result = calculate_coupling(&g, &file_id, None);
        assert_eq!(result.efferent, 1); // a.py imports b.py
        assert_eq!(result.afferent, 0); // nobody imports a.py
        assert_eq!(result.total(), 1);
    }

    #[test]
    fn coupling_result_total() {
        let r = CouplingResult {
            afferent: 3,
            efferent: 7,
        };
        assert_eq!(r.total(), 10);
    }

    #[test]
    fn instability_all_efferent() {
        let g = linear_chain();
        assert_eq!(calculate_instability(&g, "File:a.py"), 1.0);
    }

    #[test]
    fn instability_zero_coupling() {
        let mut g = ModuleDependencyGraph::new("isolated.py");
        g.add_node(file_node("File:isolated.py", "isolated.py"));
        assert_eq!(calculate_instability(&g, "File:isolated.py"), 0.5);
    }

    #[test]
    fn instability_from_precomputed_coupling() {
        let result = CouplingResult {
            afferent: 3,
            efferent: 1,
        };
        assert_eq!(calculate_instability_from_result(&result), 0.25);
    }

    #[test]
    fn dependency_depth_linear() {
        let g = linear_chain();
        assert_eq!(calculate_dependency_depth(&g, "File:a.py"), 3); // a -> b -> c -> d
    }

    #[test]
    fn dependency_depth_isolated() {
        let mut g = ModuleDependencyGraph::new("lone.py");
        g.add_node(file_node("File:lone.py", "lone.py"));
        assert_eq!(calculate_dependency_depth(&g, "File:lone.py"), 0);
    }

    /// Cycles should not cause infinite loops.
    #[test]
    fn dependency_depth_cycle() {
        let mut g = ModuleDependencyGraph::new("x.py");
        g.add_node(file_node("File:x.py", "x.py"));
        g.add_node(file_node("File:y.py", "y.py"));
        g.add_relationship(rel("i1", "File:x.py", "File:y.py", "IMPORTS"));
        g.add_relationship(rel("i2", "File:y.py", "File:x.py", "IMPORTS"));
        assert_eq!(calculate_dependency_depth(&g, "File:x.py"), 1);
    }

    /// A Function node that has no CONTAINS edge returns `None` from
    /// `owning_file`.
    #[test]
    fn owning_file_symbol_with_no_contains_parent() {
        let mut g = ModuleDependencyGraph::new("orphan.py");
        g.add_node(GraphNode {
            id: "Func:orphan:stray".to_string(),
            label: "Function".to_string(),
            properties: std::collections::HashMap::new(),
        });
        assert_eq!(owning_file(&g, "Func:orphan:stray"), None);
    }

    #[test]
    fn owning_file_unknown_node() {
        let g = ModuleDependencyGraph::new("x.py");
        assert_eq!(owning_file(&g, "nonexistent-id"), None);
    }

    /// A Function reachable via a CONTAINS edge resolves to its File
    /// owner.
    #[test]
    fn owning_file_via_contains_edge() {
        let mut g = ModuleDependencyGraph::new("owner.py");
        g.add_node(file_node("File:owner.py", "owner.py"));
        g.add_node(GraphNode {
            id: "Func:owner:fn".to_string(),
            label: "Function".to_string(),
            properties: std::collections::HashMap::new(),
        });
        g.add_relationship(rel("c1", "File:owner.py", "Func:owner:fn", "CONTAINS"));
        assert_eq!(
            owning_file(&g, "Func:owner:fn"),
            Some("File:owner.py".to_string())
        );
    }

    /// A CONTAINS cycle must not hang; `owning_file` returns `None`.
    #[test]
    fn owning_file_contains_cycle_returns_none() {
        let mut g = ModuleDependencyGraph::new("x.py");
        g.add_node(GraphNode {
            id: "A".to_string(),
            label: "Class".to_string(),
            properties: std::collections::HashMap::new(),
        });
        g.add_node(GraphNode {
            id: "B".to_string(),
            label: "Class".to_string(),
            properties: std::collections::HashMap::new(),
        });
        g.add_relationship(rel("c1", "A", "B", "CONTAINS"));
        g.add_relationship(rel("c2", "B", "A", "CONTAINS"));
        assert_eq!(owning_file(&g, "A"), None);
        assert_eq!(owning_file(&g, "B"), None);
    }
}
