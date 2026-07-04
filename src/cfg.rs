use crate::uast::UASTNode;
use petgraph::prelude::*;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    UNCONDITIONAL,
    TRUE,
    FALSE,
    LOOPBACK,
    BREAK,
    CONTINUE,
    RETURN,
    EXCEPTION,
    SWITCHCASE,
}

#[pyclass(get_all, from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BasicBlock {
    pub id: usize,
    pub statements: Vec<UASTNode>,
    pub label: String,
}

#[pymethods]
impl BasicBlock {
    #[new]
    #[pyo3(signature = (id, statements=None, label=String::new()))]
    fn new(id: usize, statements: Option<Vec<UASTNode>>, label: String) -> Self {
        Self {
            id,
            statements: statements.unwrap_or_default(),
            label,
        }
    }
}

#[pyclass(get_all, from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CFGEdge {
    pub source: usize,
    pub target: usize,
    pub kind: EdgeKind,
}

#[pymethods]
impl CFGEdge {
    #[new]
    fn new(source: usize, target: usize, kind: EdgeKind) -> Self {
        Self {
            source,
            target,
            kind,
        }
    }
}

#[pyclass(get_all, from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ControlFlowGraph {
    pub blocks: HashMap<usize, BasicBlock>,
    pub edges: Vec<CFGEdge>,
    pub entry_id: usize,
    pub exit_id: usize,
}

#[pymethods]
impl ControlFlowGraph {
    #[new]
    #[pyo3(signature = (blocks=None, edges=None, entry_id=0, exit_id=1))]
    fn new(
        blocks: Option<HashMap<usize, BasicBlock>>,
        edges: Option<Vec<CFGEdge>>,
        entry_id: usize,
        exit_id: usize,
    ) -> Self {
        Self {
            blocks: blocks.unwrap_or_default(),
            edges: edges.unwrap_or_default(),
            entry_id,
            exit_id,
        }
    }

    pub fn cyclomatic_complexity(&self) -> usize {
        let n = self.blocks.len();
        let e = self.edges.len();
        let p = self.connected_components();
        // E - N + 2P
        let result = (e as i64) - (n as i64) + 2 * (p as i64);
        if result > 1 {
            result as usize
        } else {
            1
        }
    }

    pub fn essential_complexity(&self) -> usize {
        let mut unstructured = 0;
        for edge in &self.edges {
            match edge.kind {
                EdgeKind::BREAK | EdgeKind::CONTINUE | EdgeKind::RETURN => {
                    unstructured += 1;
                }
                _ => {}
            }
        }
        (unstructured + 1).max(1) as usize
    }

    pub fn max_nesting_depth(&self) -> usize {
        let mut depth: HashMap<usize, usize> = HashMap::new();
        depth.insert(self.entry_id, 0);

        let mut changed = true;
        let mut iterations = 0;
        let max_iters = self.blocks.len() * 2;

        while changed && iterations < max_iters {
            changed = false;
            for edge in &self.edges {
                if let Some(&current_depth) = depth.get(&edge.source) {
                    let inc = match edge.kind {
                        EdgeKind::TRUE | EdgeKind::SWITCHCASE => 1,
                        _ => 0,
                    };
                    let candidate = current_depth + inc;
                    let target_depth = depth.entry(edge.target).or_insert(0);
                    if candidate > *target_depth {
                        *target_depth = candidate;
                        changed = true;
                    }
                }
            }
            iterations += 1;
        }
        *depth.values().max().unwrap_or(&0)
    }

    pub fn longest_acyclic_path(&self) -> usize {
        // LOOPBACK and CONTINUE are the only edge kinds the builder ever uses
        // to jump backward to an already-visited block (both target a loop
        // header). Stripping them makes the remaining graph a true DAG, so a
        // topological-sort DP replaces what used to be exponential path
        // enumeration. See src/cfg.rs test `test_longest_acyclic_path_panics_on_untagged_cycle`
        // for what happens if that invariant is ever broken.
        let mut graph = DiGraph::<usize, ()>::new();
        let mut indices: HashMap<usize, NodeIndex> = HashMap::new();
        for &id in self.blocks.keys() {
            indices.insert(id, graph.add_node(id));
        }
        for edge in &self.edges {
            if matches!(edge.kind, EdgeKind::LOOPBACK | EdgeKind::CONTINUE) {
                continue;
            }
            if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
                graph.add_edge(s, t, ());
            }
        }

        let order = petgraph::algo::toposort(&graph, None).expect(
            "CFG has a back-edge not tagged LOOPBACK/CONTINUE — builder invariant violated",
        );

        let Some(&entry_idx) = indices.get(&self.entry_id) else {
            return 0;
        };
        let mut dist: HashMap<NodeIndex, usize> = HashMap::new();
        dist.insert(entry_idx, 0);
        for node in order {
            let Some(&d) = dist.get(&node) else {
                continue;
            };
            for succ in graph.neighbors(node) {
                let candidate = d + 1;
                let entry = dist.entry(succ).or_insert(0);
                if candidate > *entry {
                    *entry = candidate;
                }
            }
        }

        indices
            .get(&self.exit_id)
            .and_then(|idx| dist.get(idx))
            .copied()
            .unwrap_or(0)
    }

    fn connected_components(&self) -> usize {
        let mut graph = UnGraph::<usize, ()>::default();
        let mut indices = HashMap::new();

        for &id in self.blocks.keys() {
            let idx = graph.add_node(id);
            indices.insert(id, idx);
        }

        for edge in &self.edges {
            if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
                graph.add_edge(s, t, ());
            }
        }

        petgraph::algo::connected_components(&graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies cyclomatic complexity calculation (E - N + 2P) for a linear graph with no branches (returns 1).
    #[test]
    fn test_cyclomatic_complexity_linear() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "exit".to_string()));

        let edges = vec![CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL)];

        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 1);
        assert_eq!(cfg.cyclomatic_complexity(), 1);
    }

    /// Verifies cyclomatic complexity for a branching structure (IF-ELSE) representing multiple execution paths (returns 2).
    #[test]
    fn test_cyclomatic_complexity_if() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "if".to_string()));
        blocks.insert(2, BasicBlock::new(2, None, "else".to_string()));
        blocks.insert(3, BasicBlock::new(3, None, "exit".to_string()));

        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::TRUE),
            CFGEdge::new(0, 2, EdgeKind::FALSE),
            CFGEdge::new(1, 3, EdgeKind::UNCONDITIONAL),
            CFGEdge::new(2, 3, EdgeKind::UNCONDITIONAL),
        ];

        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 3);
        // E = 4, N = 4, P = 1 => 4 - 4 + 2*1 = 2
        assert_eq!(cfg.cyclomatic_complexity(), 2);
    }

    /// Tests essential complexity logic, ensuring that unstructured control flow (like RETURN) correctly increments the complexity.
    #[test]
    fn test_essential_complexity() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "exit".to_string()));

        let edges = vec![CFGEdge::new(0, 1, EdgeKind::RETURN)];

        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 1);
        assert_eq!(cfg.essential_complexity(), 2);
    }

    /// Checks the maximum nesting depth calculation by traversing sequential nested TRUE branches.
    #[test]
    fn test_max_nesting_depth() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "inner".to_string()));
        blocks.insert(2, BasicBlock::new(2, None, "exit".to_string()));

        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::TRUE),
            CFGEdge::new(1, 2, EdgeKind::TRUE),
        ];

        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 2);
        assert_eq!(cfg.max_nesting_depth(), 2);
    }

    /// Tests the DAG longest-path calculation on a straightforward forward-flowing graph.
    #[test]
    fn test_longest_acyclic_path() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "b1".to_string()));
        blocks.insert(2, BasicBlock::new(2, None, "exit".to_string()));

        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL),
            CFGEdge::new(1, 2, EdgeKind::UNCONDITIONAL),
        ];

        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 2);
        assert_eq!(cfg.longest_acyclic_path(), 2);
    }
}
