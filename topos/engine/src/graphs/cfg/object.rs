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

    /// Maximum static nesting depth along forward control flow.
    ///
    /// Counts `True` / `SwitchCase` edges on the acyclic CFG (loop
    /// back-edges stripped — see [`Self::is_back_edge`]). Without that
    /// strip, Bellman-Ford-style relaxation re-increments through
    /// `Loopback`/`Continue` cycles and climbs to the iteration cap
    /// (`≈ 2|V|`) instead of the real nest depth (issue #288).
    ///
    /// Untagged residual cycles degrade to `0`, matching
    /// [`Self::longest_acyclic_path`].
    pub fn max_nesting_depth(&self) -> usize {
        // Edge weight = nesting increment contributed by that edge.
        let Some((graph, indices)) = self.forward_cfg_dag(|edge| {
            matches!(edge.kind, EdgeKind::True | EdgeKind::SwitchCase) as usize
        }) else {
            return 0;
        };

        let Ok(order) = petgraph::algo::toposort(&graph, None) else {
            return 0;
        };

        let Some(&entry_idx) = indices.get(&self.entry_id) else {
            return 0;
        };
        let mut depth: HashMap<NodeIndex, usize> = HashMap::new();
        depth.insert(entry_idx, 0);
        for node in order {
            let Some(&d) = depth.get(&node) else {
                continue;
            };
            for edge_ref in graph.edges(node) {
                let candidate = d + *edge_ref.weight();
                let entry = depth.entry(edge_ref.target()).or_insert(0);
                if candidate > *entry {
                    *entry = candidate;
                }
            }
        }

        depth.values().copied().max().unwrap_or(0)
    }

    /// Longest acyclic entry→exit path length (edge count).
    ///
    /// `Loopback` / `Continue` are stripped so the remaining graph is a
    /// DAG and a topological DP replaces exponential path enumeration.
    /// Untagged cycles degrade to `0` rather than panic — a single
    /// unusual CFG shape must not crash `evaluate` / `inspect`.
    pub fn longest_acyclic_path(&self) -> usize {
        let Some((graph, indices)) = self.forward_cfg_dag(|_| ()) else {
            return 0;
        };

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

    /// Builder back-edges that close loop cycles (body/continue → header).
    fn is_back_edge(kind: EdgeKind) -> bool {
        matches!(kind, EdgeKind::Loopback | EdgeKind::Continue)
    }

    /// Forward-only CFG as a `petgraph` DAG. Edge payloads come from
    /// `edge_weight`. Returns `None` only if the block map is empty.
    /// Callers must still handle `toposort` failure for untagged cycles.
    fn forward_cfg_dag<W, F>(
        &self,
        mut edge_weight: F,
    ) -> Option<(DiGraph<usize, W>, HashMap<usize, NodeIndex>)>
    where
        F: FnMut(&CFGEdge) -> W,
    {
        if self.blocks.is_empty() {
            return None;
        }
        let mut graph = DiGraph::<usize, W>::new();
        let mut indices: HashMap<usize, NodeIndex> = HashMap::new();
        for &id in self.blocks.keys() {
            indices.insert(id, graph.add_node(id));
        }
        for edge in &self.edges {
            if Self::is_back_edge(edge.kind) {
                continue;
            }
            if let (Some(&s), Some(&t)) = (indices.get(&edge.source), indices.get(&edge.target)) {
                graph.add_edge(s, t, edge_weight(edge));
            }
        }
        Some((graph, indices))
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

    /// Issue #288: branch inside a loop must not re-count nesting through
    /// the Loopback edge. Shape mirrors `for { if cond { body } }`.
    #[test]
    fn max_nesting_depth_if_inside_loop_is_one() {
        // entry → header -True→ then → join -Loopback→ header
        //                 -False→ after → exit
        //                         then -Uncond→ join
        let cfg = ControlFlowGraph::new(
            blocks_from(&[
                (0, "entry"),
                (1, "header"),
                (2, "then"),
                (3, "join"),
                (4, "after"),
                (5, "exit"),
            ]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::Unconditional),
                CFGEdge::new(1, 2, EdgeKind::True),
                CFGEdge::new(1, 4, EdgeKind::False),
                CFGEdge::new(2, 3, EdgeKind::Unconditional),
                CFGEdge::new(3, 1, EdgeKind::Loopback),
                CFGEdge::new(4, 5, EdgeKind::Unconditional),
            ],
            0,
            5,
        );
        assert_eq!(cfg.max_nesting_depth(), 1);
    }

    /// Nested ifs inside a loop: only the two True edges count.
    #[test]
    fn max_nesting_depth_nested_ifs_inside_loop() {
        // header -True→ outer_then -True→ inner_then → join -Loopback→ header
        //         -False→ after
        //                   outer_then -False→ join
        let cfg = ControlFlowGraph::new(
            blocks_from(&[
                (0, "entry"),
                (1, "header"),
                (2, "outer_then"),
                (3, "inner_then"),
                (4, "join"),
                (5, "after"),
                (6, "exit"),
            ]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::Unconditional),
                CFGEdge::new(1, 2, EdgeKind::True),
                CFGEdge::new(1, 5, EdgeKind::False),
                CFGEdge::new(2, 3, EdgeKind::True),
                CFGEdge::new(2, 4, EdgeKind::False),
                CFGEdge::new(3, 4, EdgeKind::Unconditional),
                CFGEdge::new(4, 1, EdgeKind::Loopback),
                CFGEdge::new(5, 6, EdgeKind::Unconditional),
            ],
            0,
            6,
        );
        assert_eq!(cfg.max_nesting_depth(), 2);
    }

    /// Switch arm inside a loop increments once (SwitchCase), not via Loopback.
    #[test]
    fn max_nesting_depth_switch_inside_loop() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[
                (0, "entry"),
                (1, "header"),
                (2, "arm"),
                (3, "join"),
                (4, "after"),
                (5, "exit"),
            ]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::Unconditional),
                CFGEdge::new(1, 2, EdgeKind::SwitchCase),
                CFGEdge::new(1, 4, EdgeKind::False),
                CFGEdge::new(2, 3, EdgeKind::Unconditional),
                CFGEdge::new(3, 1, EdgeKind::Loopback),
                CFGEdge::new(4, 5, EdgeKind::Unconditional),
            ],
            0,
            5,
        );
        assert_eq!(cfg.max_nesting_depth(), 1);
    }

    /// Continue back-edge must not inflate depth either.
    #[test]
    fn max_nesting_depth_ignores_continue_back_edge() {
        let cfg = ControlFlowGraph::new(
            blocks_from(&[(0, "entry"), (1, "header"), (2, "body"), (3, "exit")]),
            vec![
                CFGEdge::new(0, 1, EdgeKind::Unconditional),
                CFGEdge::new(1, 2, EdgeKind::True),
                CFGEdge::new(1, 3, EdgeKind::False),
                CFGEdge::new(2, 1, EdgeKind::Continue),
            ],
            0,
            3,
        );
        assert_eq!(cfg.max_nesting_depth(), 1);
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
    fn max_nesting_depth_returns_zero_on_untagged_cycle() {
        let blocks = blocks_from(&[(0, "a"), (1, "b"), (2, "c")]);
        let edges = vec![
            CFGEdge::new(0, 1, EdgeKind::True),
            CFGEdge::new(1, 2, EdgeKind::True),
            CFGEdge::new(2, 0, EdgeKind::True),
        ];
        let cfg = ControlFlowGraph::new(blocks, edges, 0, 2);
        assert_eq!(cfg.max_nesting_depth(), 0);
    }

    #[test]
    fn from_uast_empty_file_is_trivially_connected() {
        use crate::graphs::ast::dispatch::parse_source;
        let result = parse_source("", "python", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 1);
    }

    #[test]
    fn from_uast_survives_deeply_nested_supported_languages() {
        use crate::graphs::ast::dispatch::parse_source;
        const DEPTH: usize = 10_000;
        let open = "(".repeat(DEPTH);
        let close = ")".repeat(DEPTH);
        let cases = [
            ("python", format!("x = {open}1{close}\n")),
            ("rust", format!("const X: i32 = {open}1{close};\n")),
            ("javascript", format!("const x = {open}1{close};\n")),
            ("typescript", format!("const x: number = {open}1{close};\n")),
            ("cpp", format!("int x = {open}1{close};\n")),
            ("go", format!("package p\nvar x = {open}1{close}\n")),
        ];

        for (language, source) in cases {
            let result = parse_source(&source, language, None).unwrap();
            let cfg = ControlFlowGraph::from_uast(&result.uast_root);

            assert!(!cfg.blocks.is_empty(), "failed for {language}");
        }
    }

    #[test]
    fn from_uast_survives_deeply_nested_control_flow() {
        use crate::graphs::ast::dispatch::parse_source;
        // Nested parens (above) exercise parse/clone/drop but contain no
        // control flow; nested `if`s drive the CFG builder's continuation
        // machinery itself, which is what must stay stack-safe. The UAST
        // shape is language-neutral past the mapper, so one language
        // suffices. Depth is bounded by cost (block statements clone
        // their subtree, so nested control flow is quadratic to build);
        // the deliberately small thread stack is what makes this depth a
        // hard stack-safety gate — a recursive builder overflows 512 KiB
        // immediately, while the iterative machine stays on the heap.
        const DEPTH: usize = 1_000;
        let mut source = String::from("function f(x) {\n");
        source.push_str(&"if (x) { ".repeat(DEPTH));
        source.push_str("x = 1;");
        source.push_str(&" }".repeat(DEPTH));
        source.push_str("\n}\n");

        let cyclomatic = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                let result = parse_source(&source, "javascript", None).unwrap();
                let cfg = ControlFlowGraph::from_uast(&result.uast_root);
                cfg.cyclomatic_complexity()
            })
            .expect("spawn test thread")
            .join()
            .expect("deeply nested control flow must not overflow the stack");

        assert_eq!(cyclomatic, DEPTH + 1);
    }

    #[test]
    fn from_uast_single_if_has_cyclomatic_two() {
        use crate::graphs::ast::dispatch::parse_source;
        let source = "def f(x):\n    if x:\n        return 1\n    return 0\n";
        let result = parse_source(source, "python", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 2);
    }

    #[test]
    fn from_uast_python_match_counts_each_arm() {
        use crate::graphs::ast::dispatch::parse_source;
        let source = "def f(x):\n    match x:\n        case 1:\n            y = 1\n        case 2:\n            y = 2\n        case _:\n            y = 3\n    return y\n";
        let result = parse_source(source, "python", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        // 3 case arms => 3 branches (was 1 before `match_statement` was mapped).
        assert_eq!(cfg.cyclomatic_complexity(), 3);
    }

    #[test]
    fn from_uast_javascript_switch_counts_each_arm() {
        use crate::graphs::ast::dispatch::parse_source;
        let source = "function f(x) {\n  let y;\n  switch (x) {\n    case 1: y = 1;\n    case 2: y = 2;\n    default: y = 3;\n  }\n  return y;\n}\n";
        let result = parse_source(source, "javascript", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 3);
    }

    #[test]
    fn from_uast_cpp_switch_counts_each_arm() {
        use crate::graphs::ast::dispatch::parse_source;
        let source = "int f(int x) {\n    int y;\n    switch (x) {\n        case 1: y = 1;\n        case 2: y = 2;\n        default: y = 3;\n    }\n    return y;\n}\n";
        let result = parse_source(source, "cpp", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        assert_eq!(cfg.cyclomatic_complexity(), 3);
    }

    #[test]
    fn from_uast_go_discriminantless_switch_counts_each_case_once() {
        use crate::graphs::ast::dispatch::parse_source;
        // Regression: the discriminant-less Go switch used to flatten each
        // case's statements into separate branches (over-count).
        let source = "package p\nfunc f(x int) int {\n\tvar y int\n\tswitch {\n\tcase x > 2:\n\t\ty = 1\n\t\ty = 11\n\tcase x > 1:\n\t\ty = 2\n\t\ty = 22\n\tdefault:\n\t\ty = 3\n\t}\n\treturn y\n}\n";
        let result = parse_source(source, "go", None).unwrap();
        let cfg = ControlFlowGraph::from_uast(&result.uast_root);
        // 3 case arms => exactly 3 SwitchCase branches, not one per statement
        // (regression: the discriminant-less Go switch used to flatten each
        // case's statements into separate branches). Cyclomatic itself also
        // picks up Go's module-level `package` callable, so assert the branch
        // count directly.
        let switch_branches = cfg
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::SwitchCase)
            .count();
        assert_eq!(switch_branches, 3);
    }
}
