//! `ControlFlowGraph` — the CFG translational functor's image in the
//! topos `E`. Implements [`Representation`] and feeds the SIMPLE
//! generator of `H`.
//!
//! Metrics emitted (namespace `cfg.*`):
//! - `cfg.cyclomatic` — McCabe complexity (`E - N + 2P`, with `P = 1`).
//! - `cfg.essential` — essential complexity (structured-decomposition
//!   reduction).
//! - `cfg.nesting_depth` — maximum static nesting depth.
//! - `cfg.longest_path` — longest acyclic path (a rough proxy for path
//!   explosion).
//!
//! The metric algorithms here predate the v0.4.0 migration — they were
//! already hand-written in Rust (as a `topos-pyo3` probe, since Python
//! delegated this hot path to the Rust extension even before this
//! migration started) and are relocated unchanged in logic, only
//! stripped of `pyo3`/`serde` annotations `topos-core` doesn't need.

use std::collections::HashMap;

use petgraph::prelude::*;

use super::builder::build_cfg_from_uast;
use super::models::{Blocks, CFGEdge, EdgeKind};
use crate::graphs::base::Representation;
use crate::graphs::uast::models::UASTNode;

/// A language-independent control-flow graph built on UAST.
///
/// Construct via [`ControlFlowGraph::from_uast`] for a fully-populated
/// graph, or build `blocks`/`edges` directly for tests.
#[derive(Debug, Clone, Default)]
pub struct ControlFlowGraph {
    pub blocks: Blocks,
    pub edges: Vec<CFGEdge>,
    pub entry_id: usize,
    pub exit_id: usize,
}

impl ControlFlowGraph {
    pub fn new(blocks: Blocks, edges: Vec<CFGEdge>, entry_id: usize, exit_id: usize) -> Self {
        ControlFlowGraph {
            blocks,
            edges,
            entry_id,
            exit_id,
        }
    }

    /// Build a CFG from a UAST root, covering every callable.
    pub fn from_uast(uast_root: &UASTNode) -> Self {
        let (blocks, edges, entry_id, exit_id) = build_cfg_from_uast(uast_root);
        ControlFlowGraph::new(blocks, edges, entry_id, exit_id)
    }

    // --- Graph queries ---------------------------------------------------

    pub fn successors(&self, block_id: usize) -> Vec<&CFGEdge> {
        self.edges.iter().filter(|e| e.source == block_id).collect()
    }

    pub fn predecessors(&self, block_id: usize) -> Vec<&CFGEdge> {
        self.edges.iter().filter(|e| e.target == block_id).collect()
    }

    // --- Metrics, delegated to Rust for performance (this crate no
    // longer needs to "delegate" — it just *is* the Rust side now) -------

    pub fn cyclomatic_complexity(&self) -> usize {
        let n = self.blocks.len() as i64;
        let e = self.edges.len() as i64;
        let p = self.connected_components() as i64;
        // E - N + 2P
        let result = e - n + 2 * p;
        if result > 1 {
            result as usize
        } else {
            1
        }
    }

    pub fn essential_complexity(&self) -> usize {
        let unstructured = self
            .edges
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EdgeKind::Break | EdgeKind::Continue | EdgeKind::Return
                )
            })
            .count();
        (unstructured + 1).max(1)
    }

    pub fn max_nesting_depth(&self) -> usize {
        let mut depth: HashMap<usize, usize> = HashMap::new();
        depth.insert(self.entry_id, 0);

        let mut changed = true;
        let max_iters = self.blocks.len() * 2;
        let mut iterations = 0;
        while changed && iterations < max_iters {
            changed = false;
            for edge in &self.edges {
                if let Some(&current_depth) = depth.get(&edge.source) {
                    let inc = matches!(edge.kind, EdgeKind::True | EdgeKind::SwitchCase) as usize;
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
        depth.values().copied().max().unwrap_or(0)
    }

    /// `Loopback` and `Continue` are the only edge kinds the builder
    /// ever uses to jump backward to an already-visited block (both
    /// target a loop header). Stripping them makes the remaining graph a
    /// true DAG, so a topological-sort DP replaces what used to be
    /// exponential path enumeration.
    ///
    /// If that invariant is ever violated by an edge case the builder
    /// doesn't tag correctly, we degrade to `0` rather than panic — see
    /// the `longest_acyclic_path_returns_zero_on_untagged_cycle` test
    /// below. A single file with an unusual control-flow shape shouldn't
    /// be able to crash a whole `evaluate`/`inspect` run.
    pub fn longest_acyclic_path(&self) -> usize {
        let mut graph = DiGraph::<usize, ()>::new();
        let mut indices: HashMap<usize, NodeIndex> = HashMap::new();
        for &id in self.blocks.keys() {
            indices.insert(id, graph.add_node(id));
        }
        for edge in &self.edges {
            if matches!(edge.kind, EdgeKind::Loopback | EdgeKind::Continue) {
                continue;
            }
            if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
                graph.add_edge(s, t, ());
            }
        }

        let Ok(order) = petgraph::algo::toposort(&graph, None) else {
            return 0;
        };

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
            indices.insert(id, graph.add_node(id));
        }
        for edge in &self.edges {
            if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
                graph.add_edge(s, t, ());
            }
        }
        petgraph::algo::connected_components(&graph)
    }
}

impl Representation for ControlFlowGraph {
    fn name(&self) -> &str {
        "cfg"
    }

    /// SIMPLE generator of `H(G_qual)`.
    fn dimension(&self) -> &str {
        "simple"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        HashMap::from([
            (
                "cfg.cyclomatic".to_string(),
                self.cyclomatic_complexity() as f64,
            ),
            (
                "cfg.essential".to_string(),
                self.essential_complexity() as f64,
            ),
            (
                "cfg.nesting_depth".to_string(),
                self.max_nesting_depth() as f64,
            ),
            (
                "cfg.longest_path".to_string(),
                self.longest_acyclic_path() as f64,
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::cfg::models::BasicBlock;

    fn blocks_from(pairs: &[(usize, &str)]) -> Blocks {
        pairs
            .iter()
            .map(|&(id, label)| (id, BasicBlock::new(id, label)))
            .collect()
    }

    #[test]
    fn metrics_never_leaks_graphify_keys() {
        // Graphify is advisory-only (issue #150) and must never leak into a
        // scored Representation's metrics.
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "exit")]),
            vec![CFGEdge::new(0, 1, EdgeKind::Unconditional)],
            0,
            1,
        );
        assert!(cfg.metrics().keys().all(|k| !k.starts_with("graphify")));
    }

    #[test]
    fn cyclomatic_complexity_linear() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "exit")]),
            vec![CFGEdge::new(0, 1, EdgeKind::Unconditional)],
            0,
            1,
        );
        assert_eq!(cfg.cyclomatic_complexity(), 1);
    }

    #[test]
    fn cyclomatic_complexity_if_else() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "if"), (2, "else"), (3, "exit")]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::True),
                CFGEdge::new(0, 2, EdgeKind::False),
                CFGEdge::new(1, 3, EdgeKind::Unconditional),
                CFGEdge::new(2, 3, EdgeKind::Unconditional),
            ],
            0,
            3,
        );
        // E = 4, N = 4, P = 1 => 4 - 4 + 2*1 = 2
        assert_eq!(cfg.cyclomatic_complexity(), 2);
    }

    #[test]
    fn essential_complexity_counts_unstructured_edges() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "exit")]),
            vec![CFGEdge::new(0, 1, EdgeKind::Return)],
            0,
            1,
        );
        assert_eq!(cfg.essential_complexity(), 2);
    }

    #[test]
    fn max_nesting_depth_of_sequential_true_branches() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "inner"), (2, "exit")]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::True),
                CFGEdge::new(1, 2, EdgeKind::True),
            ],
            0,
            2,
        );
        assert_eq!(cfg.max_nesting_depth(), 2);
    }

    #[test]
    fn longest_acyclic_path_forward_flow() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "b1"), (2, "exit")]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::Unconditional),
                CFGEdge::new(1, 2, EdgeKind::Unconditional),
            ],
            0,
            2,
        );
        assert_eq!(cfg.longest_acyclic_path(), 2);
    }

    /// Regression test for issue #113: `k` sequential if/else diamonds
    /// used to force `2^k` path enumerations (hang); the DAG-DP
    /// implementation stays `O(V+E)`.
    #[test]
    fn longest_acyclic_path_many_sequential_branches() {
        let k = 40;
        let mut blocks = Blocks::new();
        let mut edges = Vec::new();
        let mut next_id = 0usize;

        blocks.insert(0, BasicBlock::new(0, "entry"));
        next_id += 1;
        let mut current_split = 0usize;

        for _ in 0..k {
            let true_id = next_id;
            next_id += 1;
            let false_id = next_id;
            next_id += 1;
            let join_id = next_id;
            next_id += 1;
            blocks.insert(true_id, BasicBlock::new(true_id, ""));
            blocks.insert(false_id, BasicBlock::new(false_id, ""));
            blocks.insert(join_id, BasicBlock::new(join_id, ""));

            edges.push(CFGEdge::new(current_split, true_id, EdgeKind::True));
            edges.push(CFGEdge::new(current_split, false_id, EdgeKind::False));
            edges.push(CFGEdge::new(true_id, join_id, EdgeKind::Unconditional));
            edges.push(CFGEdge::new(false_id, join_id, EdgeKind::Unconditional));

            current_split = join_id;
        }
        let exit_id = current_split;

        let cfg = ControlFlowGraph::new(blocks, edges, 0, exit_id);
        assert_eq!(cfg.longest_acyclic_path(), 2 * k);
    }

    #[test]
    fn longest_acyclic_path_returns_zero_on_untagged_cycle() {
        let blocks = blocks_from(&[(0, "a"), (1, "b"), (2, "c")]);
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::True),
            CFGEdge::new(1, 2, EdgeKind::True),
            CFGEdge::new(2, 0, EdgeKind::True),
        ];
        let cfg = ControlFlowGraph::new(blocks, edges, 0, 2);
        assert_eq!(cfg.longest_acyclic_path(), 0);
    }

    #[test]
    fn from_uast_empty_file_is_trivially_connected() {
        use crate::graphs::ast::dispatch::parse_source;
        let result = parse_source("", "python", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 1);
    }

    #[test]
    fn from_uast_single_if_has_cyclomatic_two() {
        use crate::graphs::ast::dispatch::parse_source;
        let source = "def f(x):\n    if x:\n        return 1\n    return 0\n";
        let result = parse_source(source, "python", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 2);
    }
}
