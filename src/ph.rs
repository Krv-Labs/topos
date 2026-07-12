//! Cycle-basis extraction for the CFG (issue #83: point cyclomatic complexity's
//! cycle count at the actual code that holds each cycle).
//!
//! Cyclomatic complexity (`E - N + 2P`, `ControlFlowGraph::cyclomatic_complexity`
//! in `cfg.rs`) already *is* the rank of the CFG's cycle space (Betti number
//! `dim H1`) — the gap this module fills is not another count, it's a basis of
//! actual representative cycles that can be mapped back to source line ranges.
//!
//! Algorithm: build a spanning tree/forest over the CFG's undirected
//! projection (mirroring the projection `connected_components()` already uses),
//! then every non-tree ("back") edge closes exactly one fundamental cycle —
//! the tree path between its endpoints plus the back edge itself. The number
//! of back edges equals `E - N + 2P`, giving a free cross-check against the
//! already-trusted cyclomatic complexity metric. O(V + E), no new dependencies.

use crate::cfg::{CFGEdge, ControlFlowGraph};
use petgraph::prelude::*;
use petgraph::visit::EdgeRef;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[pyclass(get_all, skip_from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CycleGenerator {
    /// Ordered walk of block ids forming the closed loop; first == last.
    pub blocks: Vec<usize>,
    /// The tree-path edges plus the one back edge, in walk order.
    pub edges: Vec<CFGEdge>,
    /// The non-tree edge that "generates" this cycle.
    pub back_edge: CFGEdge,
}

#[pyclass(get_all, skip_from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CycleBasisResult {
    /// Rank of H1 (the cycle space) — equals `cyclomatic_complexity() - 1`
    /// when the CFG has a single connected component (P=1, guaranteed by the
    /// builder's synthetic entry/exit blocks).
    pub betti_1: usize,
    pub cycles: Vec<CycleGenerator>,
}

/// Extract a fundamental cycle basis via spanning tree + back-edge closure.
/// Called from `ControlFlowGraph::cycle_basis` (see `cfg.rs`) rather than
/// defined as a second `#[pymethods]` block here, since PyO3 requires the
/// `multiple-pymethods` feature for that pattern and enabling it broke
/// `cargo test` linking on macOS/arm64 in this workspace.
pub fn compute_cycle_basis(cfg: &ControlFlowGraph) -> CycleBasisResult {
    let mut graph = UnGraph::<usize, CFGEdge>::default();
    let mut indices: HashMap<usize, NodeIndex> = HashMap::new();
    for &id in cfg.blocks.keys() {
        indices.insert(id, graph.add_node(id));
    }
    // Self-loops (a block branching to itself) aren't produced by the CFG
    // builder today and aren't given cycle representation here; every other
    // edge is added even when it duplicates a node pair already connected
    // (e.g. a loop's forward edge and its LOOPBACK edge share both
    // endpoints) — `petgraph::UnGraph` is a multigraph, so both are kept as
    // distinct edges, which is exactly what lets the BFS below tell "the
    // edge used to discover this node" apart from "a genuine back edge
    // between the same two nodes".
    for edge in &cfg.edges {
        if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
            if s == t {
                continue;
            }
            graph.add_edge(s, t, edge.clone());
        }
    }

    // BFS spanning forest, tracked by edge identity (not node-pair identity):
    // parent pointers + depth per node, and the set of `EdgeIndex`es actually
    // used to discover a node ("tree edges"). Every edge not in that set is a
    // genuine back edge, even if it connects a pair of nodes some other edge
    // also connects.
    let mut parent: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    let mut parent_edge: HashMap<NodeIndex, CFGEdge> = HashMap::new();
    let mut depth: HashMap<NodeIndex, usize> = HashMap::new();
    let mut visited: HashMap<NodeIndex, bool> = HashMap::new();
    let mut tree_edge_ids: std::collections::HashSet<EdgeIndex> = std::collections::HashSet::new();

    let roots: Vec<NodeIndex> = {
        let mut ordered = Vec::new();
        if let Some(&entry_idx) = indices.get(&cfg.entry_id) {
            ordered.push(entry_idx);
        }
        for &idx in indices.values() {
            if !ordered.contains(&idx) {
                ordered.push(idx);
            }
        }
        ordered
    };

    for &root in &roots {
        if visited.get(&root).copied().unwrap_or(false) {
            continue;
        }
        visited.insert(root, true);
        depth.insert(root, 0);
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root);
        while let Some(node) = queue.pop_front() {
            let incident: Vec<(NodeIndex, EdgeIndex, CFGEdge)> = graph
                .edges(node)
                .map(|e| (e.target(), e.id(), e.weight().clone()))
                .collect();
            for (neighbor, edge_id, edge_weight) in incident {
                if !visited.get(&neighbor).copied().unwrap_or(false) {
                    visited.insert(neighbor, true);
                    parent.insert(neighbor, node);
                    depth.insert(neighbor, depth[&node] + 1);
                    parent_edge.insert(neighbor, edge_weight);
                    tree_edge_ids.insert(edge_id);
                    queue.push_back(neighbor);
                }
            }
        }
    }

    // Every non-tree edge closes exactly one fundamental cycle.
    let mut cycles = Vec::new();
    for edge_ref in graph.edge_references() {
        if tree_edge_ids.contains(&edge_ref.id()) {
            continue;
        }
        let s = edge_ref.source();
        let t = edge_ref.target();
        cycles.push(build_cycle(
            &graph,
            &parent,
            &parent_edge,
            &depth,
            s,
            t,
            edge_ref.weight(),
        ));
    }

    let betti_1 = cycles.len();
    CycleBasisResult { betti_1, cycles }
}

/// Walk from `s` and `t` up to their lowest common ancestor in the spanning
/// tree, closing the loop with the back edge `s -> t`.
fn build_cycle(
    graph: &UnGraph<usize, CFGEdge>,
    parent: &HashMap<NodeIndex, NodeIndex>,
    parent_edge: &HashMap<NodeIndex, CFGEdge>,
    depth: &HashMap<NodeIndex, usize>,
    s: NodeIndex,
    t: NodeIndex,
    back_edge: &CFGEdge,
) -> CycleGenerator {
    // Walk s and t up to their LCA, recording each hop's connecting edge
    // (keyed by the *child* node, since that's how `parent_edge` was built)
    // alongside the node path itself.
    let mut path_s = vec![s]; // s .. lca
    let mut path_t = vec![t]; // t .. lca
    let mut edges_s = Vec::new(); // edge(s,parent(s)), edge(parent(s),..), .. up to lca
    let mut edges_t = Vec::new(); // edge(t,parent(t)), .. up to lca
    let mut cur_s = s;
    let mut cur_t = t;

    while depth[&cur_s] > depth[&cur_t] {
        edges_s.push(parent_edge[&cur_s].clone());
        cur_s = parent[&cur_s];
        path_s.push(cur_s);
    }
    while depth[&cur_t] > depth[&cur_s] {
        edges_t.push(parent_edge[&cur_t].clone());
        cur_t = parent[&cur_t];
        path_t.push(cur_t);
    }
    while cur_s != cur_t {
        edges_s.push(parent_edge[&cur_s].clone());
        cur_s = parent[&cur_s];
        path_s.push(cur_s);
        edges_t.push(parent_edge[&cur_t].clone());
        cur_t = parent[&cur_t];
        path_t.push(cur_t);
    }
    // path_s: s .. lca ; path_t: t .. lca. Closed walk: s -> ... -> lca -> ... -> t -> s.
    path_t.pop(); // drop the duplicated lca
    path_t.reverse();
    edges_t.reverse();

    let mut blocks: Vec<usize> = path_s.iter().map(|&n| graph[n]).collect();
    blocks.extend(path_t.iter().map(|&n| graph[n]));
    blocks.push(graph[s]); // close the loop

    let mut edges = edges_s;
    edges.extend(edges_t);
    edges.push(back_edge.clone());

    CycleGenerator {
        blocks,
        edges,
        back_edge: back_edge.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::{BasicBlock, EdgeKind};

    /// A linear CFG with no branches has zero cycles and betti_1 == 0.
    #[test]
    fn test_cycle_basis_linear_has_zero_cycles() {
        let mut blocks = HashMap::new();
        blocks.insert(0, BasicBlock::new(0, None, "entry".to_string()));
        blocks.insert(1, BasicBlock::new(1, None, "exit".to_string()));
        let edges = vec![CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL)];
        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 1);

        let result = cfg.cycle_basis();
        assert_eq!(result.betti_1, 0);
        assert!(result.cycles.is_empty());
    }

    /// A single `while`-shaped loop (one LOOPBACK edge) yields exactly one
    /// cycle whose block walk covers the loop body.
    #[test]
    fn test_cycle_basis_single_loop() {
        let mut blocks = HashMap::new();
        for id in 0..4 {
            blocks.insert(id, BasicBlock::new(id, None, String::new()));
        }
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL), // entry -> header
            CFGEdge::new(1, 2, EdgeKind::TRUE),          // header -> body
            CFGEdge::new(2, 1, EdgeKind::LOOPBACK),      // body -> header (back edge)
            CFGEdge::new(1, 3, EdgeKind::FALSE),         // header -> exit
        ];
        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 3);

        let result = cfg.cycle_basis();
        assert_eq!(result.betti_1, 1);
        assert_eq!(result.cycles.len(), 1);
        let cycle = &result.cycles[0];
        assert_eq!(cycle.blocks.first(), cycle.blocks.last());
        let block_set: std::collections::HashSet<_> = cycle.blocks.iter().copied().collect();
        assert!(block_set.contains(&1));
        assert!(block_set.contains(&2));
    }

    /// Two independent loops off a shared entry give betti_1 == 2.
    #[test]
    fn test_cycle_basis_nested_loops() {
        let mut blocks = HashMap::new();
        for id in 0..7 {
            blocks.insert(id, BasicBlock::new(id, None, String::new()));
        }
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL), // entry -> outer header
            CFGEdge::new(1, 2, EdgeKind::TRUE),          // outer header -> inner header
            CFGEdge::new(2, 3, EdgeKind::TRUE),          // inner header -> inner body
            CFGEdge::new(3, 2, EdgeKind::LOOPBACK),      // inner body -> inner header
            CFGEdge::new(2, 4, EdgeKind::FALSE),         // inner header -> outer body
            CFGEdge::new(4, 1, EdgeKind::LOOPBACK),      // outer body -> outer header
            CFGEdge::new(1, 5, EdgeKind::FALSE),         // outer header -> after
            CFGEdge::new(5, 6, EdgeKind::UNCONDITIONAL), // after -> exit
        ];
        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 6);

        let result = cfg.cycle_basis();
        assert_eq!(result.betti_1, 2);
        assert_eq!(result.cycles.len(), 2);
    }

    /// Cross-check: betti_1 from the cycle basis must always equal
    /// cyclomatic_complexity() - 1 (P=1 invariant guaranteed by the builder).
    #[test]
    fn test_betti_1_matches_cyclomatic_complexity_minus_one() {
        let cases: Vec<(HashMap<usize, BasicBlock>, Vec<CFGEdge>, usize, usize)> = vec![
            {
                let mut b = HashMap::new();
                b.insert(0, BasicBlock::new(0, None, String::new()));
                b.insert(1, BasicBlock::new(1, None, String::new()));
                (b, vec![CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL)], 0, 1)
            },
            {
                let mut b = HashMap::new();
                for id in 0..4 {
                    b.insert(id, BasicBlock::new(id, None, String::new()));
                }
                let e = vec![
                    CFGEdge::new(0, 1, EdgeKind::TRUE),
                    CFGEdge::new(0, 2, EdgeKind::FALSE),
                    CFGEdge::new(1, 3, EdgeKind::UNCONDITIONAL),
                    CFGEdge::new(2, 3, EdgeKind::UNCONDITIONAL),
                ];
                (b, e, 0, 3)
            },
            {
                let mut b = HashMap::new();
                for id in 0..4 {
                    b.insert(id, BasicBlock::new(id, None, String::new()));
                }
                let e = vec![
                    CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL),
                    CFGEdge::new(1, 2, EdgeKind::TRUE),
                    CFGEdge::new(2, 1, EdgeKind::LOOPBACK),
                    CFGEdge::new(1, 3, EdgeKind::FALSE),
                ];
                (b, e, 0, 3)
            },
        ];

        for (blocks, edges, entry, exit) in cases {
            let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), entry, exit);
            let cyclomatic = cfg.cyclomatic_complexity();
            let betti_1 = cfg.cycle_basis().betti_1;
            assert_eq!(betti_1, cyclomatic - 1);
        }
    }

    /// Every extracted cycle must be a genuine closed walk: first block equals
    /// last, and each consecutive pair of blocks is joined by one of the
    /// cycle's own listed edges.
    #[test]
    fn test_cycle_edges_are_closed_walk() {
        let mut blocks = HashMap::new();
        for id in 0..4 {
            blocks.insert(id, BasicBlock::new(id, None, String::new()));
        }
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::UNCONDITIONAL),
            CFGEdge::new(1, 2, EdgeKind::TRUE),
            CFGEdge::new(2, 1, EdgeKind::LOOPBACK),
            CFGEdge::new(1, 3, EdgeKind::FALSE),
        ];
        let cfg = ControlFlowGraph::new(Some(blocks), Some(edges), 0, 3);
        let result = cfg.cycle_basis();
        assert_eq!(result.cycles.len(), 1);
        let cycle = &result.cycles[0];

        assert_eq!(cycle.blocks.first(), cycle.blocks.last());
        for window in cycle.blocks.windows(2) {
            let (a, b) = (window[0], window[1]);
            let joined = cycle
                .edges
                .iter()
                .any(|e| (e.source == a && e.target == b) || (e.source == b && e.target == a));
            assert!(joined, "no cycle edge joins consecutive blocks {a} -> {b}");
        }
    }
}
