//! Academic Program Dependence Graph (Ferrante/Ottenstein style).
//!
//! An intra-procedural Program Dependence Graph composes two edge
//! families over the procedure's statement nodes:
//!
//! - **Data Dependence Graph (DDG)**: `u -DDG-> v` iff `u` defines a
//!   variable later read by `v` (no intervening redefinition).
//! - **Control Dependence Graph (CDG)**: `u -CDG-> v` iff `v` executes
//!   only when `u` takes a particular branch (computed from the CFG
//!   post-dominator tree).
//!
//! The fused graph is what slicing, taint, and program-aware diff use.
//! It implements [`Representation`] so the rest of Topos can treat it
//! uniformly. This v1 PDG is intentionally **not** a generator source
//! for the lattice (it has no `Φ` of its own); it is consumed by the CPG
//! builder. Its `dimension` is therefore set to a neutral `"composable"`
//! value so the dispatcher ignores it when there are no dedicated PDG
//! metrics — but the metrics it emits are still surfaced for
//! diagnostics.

use std::collections::{HashMap, HashSet};

use crate::graphs::base::Representation;
use crate::graphs::cfg::models::EdgeKind;
use crate::graphs::cfg::object::ControlFlowGraph;
use crate::graphs::uast::models::UASTNode;

use super::dataflow::defs_and_uses;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependenceKind {
    Data,
    Control,
}

/// A typed dependence edge between two UAST nodes by stable id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependenceEdge {
    pub source: String,
    pub target: String,
    pub kind: DependenceKind,
    /// For DATA edges: the variable name carrying the dependence.
    pub var: String,
}

impl DependenceEdge {
    fn new(source: String, target: String, kind: DependenceKind, var: impl Into<String>) -> Self {
        DependenceEdge {
            source,
            target,
            kind,
            var: var.into(),
        }
    }
}

/// Intra-procedural Program Dependence Graph.
#[derive(Debug, Clone, Default)]
pub struct ProgramDependenceGraph {
    /// All UAST statement nodes contributing to dependence.
    pub statements: Vec<UASTNode>,
    /// Typed dependence edges (DATA / CONTROL).
    pub edges: Vec<DependenceEdge>,
    /// The CFG from which control dependence was derived.
    pub cfg: Option<ControlFlowGraph>,
}

impl ProgramDependenceGraph {
    /// Construct DDG ∪ CDG using a freshly-built CFG.
    ///
    /// `source` is optional (pass `""` for backward compatibility) and,
    /// when supplied, lets data dependence recover real identifier text
    /// (see the `dataflow` module) instead of falling back to each
    /// occurrence's own node id.
    pub fn from_uast(uast_root: &UASTNode, source: &str) -> Self {
        let cfg = ControlFlowGraph::from_uast(uast_root);
        // `cfg.blocks` is a `HashMap`, whose iteration order carries no
        // relationship to program order (unlike Python's insertion-ordered
        // `dict`) -- block ids are assigned sequentially by the builder, so
        // sorting by id recovers the textual order reaching-definitions
        // analysis (below) depends on.
        let mut block_ids: Vec<&usize> = cfg.blocks.keys().collect();
        block_ids.sort();
        let mut statements: Vec<UASTNode> = Vec::new();
        for &block_id in &block_ids {
            statements.extend(cfg.blocks[block_id].statements.iter().cloned());
        }

        let mut edges = Vec::new();
        for procedure_statements in statements_by_procedure(&cfg) {
            edges.extend(compute_data_dependence(&procedure_statements, source));
        }
        edges.extend(compute_control_dependence(&cfg));

        ProgramDependenceGraph {
            statements,
            edges,
            cfg: Some(cfg),
        }
    }
}

impl Representation for ProgramDependenceGraph {
    fn name(&self) -> &str {
        "pdg"
    }

    /// The PDG does not own a generator; it surfaces diagnostic metrics
    /// under the COMPOSABLE generator (which already covers
    /// inter-statement structure for module-level dep graphs).
    fn dimension(&self) -> &str {
        "composable"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        let data = self
            .edges
            .iter()
            .filter(|e| e.kind == DependenceKind::Data)
            .count();
        let control = self
            .edges
            .iter()
            .filter(|e| e.kind == DependenceKind::Control)
            .count();
        let n = self.statements.len().max(1) as f64;
        HashMap::from([
            ("pdg.data_deps".to_string(), data as f64),
            ("pdg.control_deps".to_string(), control as f64),
            ("pdg.density".to_string(), (data + control) as f64 / n),
        ])
    }
}

// --- Internals -----------------------------------------------------------

/// Fallback key for a UAST node with no natural `id` (e.g. hand-built
/// synthetic nodes) -- matches `graphs::cpg::builder::node_key`'s
/// `anon::{ptr:x}` convention exactly (issue #159) so a DDG/CDG edge
/// referencing an anonymous statement resolves to the same key the
/// CPG's node map inserted it under. Kept as a private duplicate rather
/// than a cross-module import to avoid a `pdg -> cpg` dependency (the
/// CPG already depends on this module, not the reverse).
fn node_key(node: &UASTNode) -> String {
    if node.id.is_empty() {
        format!("anon::{:x}", std::ptr::from_ref(node) as usize)
    } else {
        node.id.clone()
    }
}

/// Approximate reaching-definitions data dependence.
///
/// Walks the statement list in textual order; for each statement records
/// the variables it defines (any `Identifier` child of an `AssignExpr`
/// in the left-hand side position) and the variables it uses (any other
/// `Identifier` descendant). A dependence edge `u -DDG-> v[var]` is
/// emitted when the most recent definer of `var` is `u`.
///
/// This is intentionally coarse — no alias analysis, no SSA, no flow
/// sensitivity. Sufficient for the security probes in v1.
fn compute_data_dependence(statements: &[UASTNode], source: &str) -> Vec<DependenceEdge> {
    let mut edges = Vec::new();
    let mut last_def: HashMap<String, String> = HashMap::new();

    for stmt in statements {
        let (defs, uses) = defs_and_uses(stmt, source);
        let stmt_id = node_key(stmt);
        for var in &uses {
            if let Some(definer) = last_def.get(var) {
                if definer != &stmt_id {
                    edges.push(DependenceEdge::new(
                        definer.clone(),
                        stmt_id.clone(),
                        DependenceKind::Data,
                        var.clone(),
                    ));
                }
            }
        }
        for var in defs {
            last_def.insert(var, stmt_id.clone());
        }
    }

    edges
}

/// Return CFG statements grouped by callable/module entry.
///
/// The CFG builder wires the synthetic entry block to one `call_*`
/// entry block per callable (and one implicit module-level callable
/// when needed). Data dependence is intra-procedural, so each group
/// gets its own reaching-definitions map instead of sharing
/// file-global variable names.
fn statements_by_procedure(cfg: &ControlFlowGraph) -> Vec<Vec<UASTNode>> {
    let procedure_entries: Vec<usize> = cfg
        .edges
        .iter()
        .filter(|edge| edge.source == cfg.entry_id && edge.target != cfg.exit_id)
        .map(|edge| edge.target)
        .collect();

    if procedure_entries.is_empty() {
        let mut block_ids: Vec<&usize> = cfg.blocks.keys().collect();
        block_ids.sort();
        let all = block_ids
            .into_iter()
            .flat_map(|id| cfg.blocks[id].statements.iter().cloned())
            .collect();
        return vec![all];
    }

    let entry_set: HashSet<usize> = procedure_entries.iter().copied().collect();
    let mut sorted_block_ids: Vec<usize> = cfg.blocks.keys().copied().collect();
    sorted_block_ids.sort();
    procedure_entries
        .iter()
        .map(|&entry_id| {
            let reachable = reachable_procedure_blocks(cfg, entry_id, &entry_set);
            let mut group = Vec::new();
            for block_id in &sorted_block_ids {
                if reachable.contains(block_id) {
                    group.extend(cfg.blocks[block_id].statements.iter().cloned());
                }
            }
            group
        })
        .collect()
}

fn reachable_procedure_blocks(
    cfg: &ControlFlowGraph,
    entry_id: usize,
    procedure_entries: &HashSet<usize>,
) -> HashSet<usize> {
    let mut reachable = HashSet::new();
    let mut stack = vec![entry_id];
    while let Some(block_id) = stack.pop() {
        if reachable.contains(&block_id) || block_id == cfg.exit_id {
            continue;
        }
        reachable.insert(block_id);
        for edge in cfg.successors(block_id) {
            if procedure_entries.contains(&edge.target) && edge.target != entry_id {
                continue;
            }
            stack.push(edge.target);
        }
    }
    reachable
}

/// Control dependence: every statement in a TRUE/FALSE/SWITCH_CASE
/// successor block is control-dependent on the predicate statement in
/// the source block.
///
/// This is a structural shortcut around the canonical post-dominator
/// algorithm. Good enough for the v1 CPG; refine later if needed.
fn compute_control_dependence(cfg: &ControlFlowGraph) -> Vec<DependenceEdge> {
    let mut edges = Vec::new();

    for edge in &cfg.edges {
        if !matches!(
            edge.kind,
            EdgeKind::True | EdgeKind::False | EdgeKind::SwitchCase
        ) {
            continue;
        }
        let Some(predicate_block) = cfg.blocks.get(&edge.source) else {
            continue;
        };
        let Some(successor_block) = cfg.blocks.get(&edge.target) else {
            continue;
        };
        let Some(predicate_stmt) = predicate_block.statements.last() else {
            continue;
        };
        let predicate_id = node_key(predicate_stmt);
        for dep_stmt in &successor_block.statements {
            let target = node_key(dep_stmt);
            if target == predicate_id {
                continue;
            }
            edges.push(DependenceEdge::new(
                predicate_id.clone(),
                target,
                DependenceKind::Control,
                "",
            ));
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;
    use crate::graphs::uast::models::{NativeRef, SourceSpan};

    #[test]
    fn node_key_uses_anon_ptr_convention_for_empty_id_nodes() {
        // Issue #159: `graphs::cpg::builder` builds its node map with an
        // identical private helper; both must produce the same
        // `anon::{ptr}` format for an anonymous (empty-id) statement, or
        // a DDG/CDG edge built here silently fails to resolve against
        // that node map.
        let anon = UASTNode {
            kind: "Anonymous".to_string(),
            lang: "python".to_string(),
            span: SourceSpan {
                file: None,
                start_byte: 0,
                end_byte: 0,
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            native: NativeRef {
                parser: "test".to_string(),
                parser_version: "0".to_string(),
                node_kind: "anon".to_string(),
            },
            attributes: HashMap::new(),
            children: Vec::new(),
            id: String::new(),
        };
        let key = node_key(&anon);
        assert!(
            key.starts_with("anon::"),
            "expected `anon::<ptr>` key for empty-id node, got {key:?}"
        );
        assert_ne!(
            key, "<anon>",
            "must not use the old literal-string convention"
        );
    }

    #[test]
    fn from_uast_computes_control_dependence_for_if() {
        let source = "def f(x):\n    if x:\n        y = 1\n    return 0\n";
        let result = parse_source(source, "python", None).unwrap();
        let pdg = ProgramDependenceGraph::from_uast(&result.uast_root, source);
        assert!(pdg.edges.iter().any(|e| e.kind == DependenceKind::Control));
    }

    /// Issue #154: without real identifier text, two occurrences of the
    /// same variable across statements were never recognized as the
    /// same dependence key -- `pdg.data_deps` was always 0.
    #[test]
    fn from_uast_recovers_data_dependence_via_source_text() {
        let source = "def f():\n    x = 1\n    y = x\n";
        let result = parse_source(source, "python", None).unwrap();
        let pdg = ProgramDependenceGraph::from_uast(&result.uast_root, source);
        assert!(
            pdg.edges
                .iter()
                .any(|e| e.kind == DependenceKind::Data && e.var == "x"),
            "expected a data-dependence edge on `x`, got {:?}",
            pdg.edges
        );
    }

    #[test]
    fn from_uast_without_source_falls_back_to_node_id() {
        let source = "def f():\n    x = 1\n    y = x\n";
        let result = parse_source(source, "python", None).unwrap();
        let pdg = ProgramDependenceGraph::from_uast(&result.uast_root, "");
        assert!(!pdg.edges.iter().any(|e| e.kind == DependenceKind::Data));
    }

    #[test]
    fn metrics_density_uses_statement_count_floor_of_one() {
        let pdg = ProgramDependenceGraph::default();
        let metrics = pdg.metrics();
        assert_eq!(metrics["pdg.data_deps"], 0.0);
        assert_eq!(metrics["pdg.density"], 0.0);
    }
}
