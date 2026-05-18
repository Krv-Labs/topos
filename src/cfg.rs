use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use crate::uast::UASTNode;
use petgraph::prelude::*;
use std::collections::HashMap;

#[pyclass(eq, eq_int)]
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

#[pyclass(get_all)]
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

#[pyclass(get_all)]
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

#[pyclass(get_all)]
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
        let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
        for edge in &self.edges {
            if edge.kind == EdgeKind::LOOPBACK {
                continue;
            }
            adj.entry(edge.source).or_default().push(edge.target);
        }

        let mut best = 0;
        let sys_block_count = self.blocks.len();
        let mut visited = std::collections::HashSet::new();
        visited.insert(self.entry_id);

        self.dfs_internal(self.entry_id, 0, &mut visited, &adj, &mut best, sys_block_count);
        best
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

impl ControlFlowGraph {
    fn dfs_internal(
        &self,
        node: usize,
        length: usize,
        visited: &mut std::collections::HashSet<usize>,
        adj: &HashMap<usize, Vec<usize>>,
        best: &mut usize,
        max_depth: usize,
    ) {
        if node == self.exit_id {
            if length > *best {
                *best = length;
            }
            return;
        }
        if length > max_depth {
            return;
        }
        if let Some(neighbors) = adj.get(&node) {
            for &nxt in neighbors {
                if visited.contains(&nxt) {
                    continue;
                }
                visited.insert(nxt);
                self.dfs_internal(nxt, length + 1, visited, adj, best, max_depth);
                visited.remove(&nxt);
            }
        }
    }
}
