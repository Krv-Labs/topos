//! CFG cycle-basis probe (issue #83: point cyclomatic complexity's cycle
//! count at the actual code that holds each cycle).
//!
//! Cyclomatic complexity (`E - N + 2P`,
//! [`ControlFlowGraph::cyclomatic_complexity`]) already *is* the rank of the
//! CFG's cycle space (Betti number `dim H1`) — the gap this module fills is
//! not another count, it's a basis of actual representative cycles mapped
//! back to source line ranges, so a refactoring tool can point directly at
//! the loop body responsible for a complexity hotspot.
//!
//! Algorithm: build a spanning tree/forest over the CFG's undirected
//! projection, then every non-tree ("back") edge closes exactly one
//! fundamental cycle — the tree path between its endpoints plus the back
//! edge itself. The number of back edges equals `E - N + 2P`, giving a free
//! cross-check against the already-trusted cyclomatic complexity metric.
//! O(V + E), no new dependencies.
//!
//! Purely advisory — never folded into `cfg.*` metrics or the SIMPLE score;
//! feeds `topos refactor cycles`. Moved from the former `topos-pyo3`
//! extension crate (`ph.rs`) per PR #159 review: computation lives in
//! `topos-core`.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::prelude::*;
use petgraph::visit::EdgeRef;

use crate::graphs::cfg::models::CFGEdge;
use crate::graphs::cfg::object::ControlFlowGraph;

/// One fundamental cycle, as a closed walk of basic-block ids.
#[derive(Clone, Debug)]
pub struct CycleGenerator {
    /// Ordered walk of block ids forming the closed loop; first == last.
    pub blocks: Vec<usize>,
    /// The tree-path edges plus the one back edge, in walk order.
    pub edges: Vec<CFGEdge>,
    /// The non-tree edge that "generates" this cycle.
    pub back_edge: CFGEdge,
}

/// A fundamental cycle basis for a CFG.
#[derive(Clone, Debug)]
pub struct CycleBasisResult {
    /// Rank of H1 (the cycle space) — equals `cyclomatic_complexity() - 1`
    /// when the CFG has a single connected component (P=1, guaranteed by the
    /// builder's synthetic entry/exit blocks).
    pub betti_1: usize,
    pub cycles: Vec<CycleGenerator>,
}

/// One cycle generator, mapped to the source range it covers.
#[derive(Clone, Debug)]
pub struct SourceCycle {
    /// The basic blocks (in walk order, closing duplicate removed) that make
    /// up this cycle.
    pub block_ids: Vec<usize>,
    /// Earliest source line covered by any block in the cycle.
    pub start_line: Option<usize>,
    /// Latest source line covered by any block in the cycle.
    pub end_line: Option<usize>,
    /// Source file path, when available.
    pub file: Option<String>,
}

/// Cycle basis for a CFG, with each cycle mapped to source lines.
#[derive(Clone, Debug, Default)]
pub struct CfgHomologyResult {
    /// Rank of the cycle space — equals `cyclomatic - 1` for the
    /// single-connected-component CFGs the builder always produces.
    pub betti_1: usize,
    pub cycles: Vec<SourceCycle>,
}

/// Extract a fundamental cycle basis via spanning tree + back-edge closure.
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
            graph.add_edge(s, t, *edge);
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
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut tree_edge_ids: HashSet<EdgeIndex> = HashSet::new();

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
        if visited.contains(&root) {
            continue;
        }
        visited.insert(root);
        depth.insert(root, 0);
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(node) = queue.pop_front() {
            let incident: Vec<(NodeIndex, EdgeIndex, CFGEdge)> = graph
                .edges(node)
                .map(|e| (e.target(), e.id(), *e.weight()))
                .collect();
            for (neighbor, edge_id, edge_weight) in incident {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
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
    let mut edges_s = Vec::new(); // edge(s,parent(s)), .. up to lca
    let mut edges_t = Vec::new(); // edge(t,parent(t)), .. up to lca
    let mut cur_s = s;
    let mut cur_t = t;

    while depth[&cur_s] > depth[&cur_t] {
        edges_s.push(parent_edge[&cur_s]);
        cur_s = parent[&cur_s];
        path_s.push(cur_s);
    }
    while depth[&cur_t] > depth[&cur_s] {
        edges_t.push(parent_edge[&cur_t]);
        cur_t = parent[&cur_t];
        path_t.push(cur_t);
    }
    while cur_s != cur_t {
        edges_s.push(parent_edge[&cur_s]);
        cur_s = parent[&cur_s];
        path_s.push(cur_s);
        edges_t.push(parent_edge[&cur_t]);
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
    edges.push(*back_edge);

    CycleGenerator {
        blocks,
        edges,
        back_edge: *back_edge,
    }
}

/// Extract a fundamental cycle basis and map each cycle to its source range.
///
/// Port of the Python glue probe `topos/functors/probes/cfg/homology.py`:
/// the walk's closing duplicate block is deduped, and each cycle's line
/// range is the min/max over its blocks' statement spans.
pub fn calculate_cycle_basis(cfg: &ControlFlowGraph) -> CfgHomologyResult {
    let raw = compute_cycle_basis(cfg);

    let mut cycles = Vec::with_capacity(raw.cycles.len());
    for cycle in &raw.cycles {
        // The walk is a closed loop (first block id == last); dedupe while
        // preserving order for a cleaner block list to report.
        let mut block_ids: Vec<usize> = Vec::new();
        for &id in &cycle.blocks {
            if !block_ids.contains(&id) {
                block_ids.push(id);
            }
        }

        let mut start_line: Option<usize> = None;
        let mut end_line: Option<usize> = None;
        let mut file: Option<String> = None;
        for block_id in &block_ids {
            let Some(block) = cfg.blocks.get(block_id) else {
                continue;
            };
            for stmt in &block.statements {
                let span = &stmt.span;
                if file.is_none() {
                    file.clone_from(&span.file);
                }
                if start_line.is_none_or(|cur| span.start_line < cur) {
                    start_line = Some(span.start_line);
                }
                if end_line.is_none_or(|cur| span.end_line > cur) {
                    end_line = Some(span.end_line);
                }
            }
        }

        cycles.push(SourceCycle {
            block_ids,
            start_line,
            end_line,
            file,
        });
    }

    CfgHomologyResult {
        betti_1: raw.betti_1,
        cycles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::morphism::ProgramMorphism;
    use crate::graphs::cfg::models::{BasicBlock, EdgeKind};

    fn linear_cfg() -> ControlFlowGraph {
        let blocks = [
            (0, BasicBlock::new(0, "entry")),
            (1, BasicBlock::new(1, "body")),
            (2, BasicBlock::new(2, "exit")),
        ]
        .into_iter()
        .collect();
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::Unconditional),
            CFGEdge::new(1, 2, EdgeKind::Unconditional),
        ];
        ControlFlowGraph::new(blocks, edges, 0, 2)
    }

    #[test]
    fn linear_cfg_has_zero_cycles() {
        let result = compute_cycle_basis(&linear_cfg());
        assert_eq!(result.betti_1, 0);
        assert!(result.cycles.is_empty());
    }

    #[test]
    fn single_loop_yields_one_cycle() {
        let blocks = [
            (0, BasicBlock::new(0, "entry")),
            (1, BasicBlock::new(1, "loop_head")),
            (2, BasicBlock::new(2, "loop_body")),
            (3, BasicBlock::new(3, "exit")),
        ]
        .into_iter()
        .collect();
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::Unconditional),
            CFGEdge::new(1, 2, EdgeKind::True),
            CFGEdge::new(2, 1, EdgeKind::Loopback),
            CFGEdge::new(1, 3, EdgeKind::False),
        ];
        let cfg = ControlFlowGraph::new(blocks, edges, 0, 3);
        let result = compute_cycle_basis(&cfg);
        assert_eq!(result.betti_1, 1);
        let cycle = &result.cycles[0];
        assert_eq!(cycle.blocks.first(), cycle.blocks.last());
        assert!(cycle.blocks.contains(&1) && cycle.blocks.contains(&2));
    }

    /// betti_1 == cyclomatic - 1 for the single-component CFGs the real
    /// builder produces (P=1: E - N + 2 = back_edges + 1).
    #[test]
    fn betti_1_matches_cyclomatic_minus_one_for_real_source() {
        let source = r#"
def f(xs):
    total = 0
    for x in xs:
        if x > 0:
            total += x
        else:
            total -= x
    while total > 10:
        total //= 2
    return total
"#;
        let mut morphism = ProgramMorphism::new(source, "python");
        let cfg = morphism.build_cfg().expect("CFG builds").clone();
        let cyclomatic = cfg.cyclomatic_complexity();
        let result = calculate_cycle_basis(&cfg);
        assert_eq!(result.betti_1, cyclomatic - 1);
        // Real-source cycles carry line ranges.
        assert!(result
            .cycles
            .iter()
            .any(|c| c.start_line.is_some() && c.end_line.is_some()));
    }

    #[test]
    fn cycle_walks_are_closed() {
        let source = "def f(n):\n    while n > 0:\n        n -= 1\n    return n\n";
        let mut morphism = ProgramMorphism::new(source, "python");
        let cfg = morphism.build_cfg().expect("CFG builds").clone();
        let result = compute_cycle_basis(&cfg);
        for cycle in &result.cycles {
            assert!(cycle.blocks.len() >= 3);
            assert_eq!(cycle.blocks.first(), cycle.blocks.last());
            assert_eq!(cycle.edges.len(), cycle.blocks.len() - 1);
        }
    }
}
