//! CFG builder — construct a [`ControlFlowGraph`](super::object::ControlFlowGraph)
//! from a UAST root.
//!
//! The builder walks the language-independent UAST kind set (`IfStmt`,
//! `ForStmt`, `WhileStmt`, `MatchStmt`, `ReturnStmt`, `BreakStmt`,
//! `ContinueStmt`, `ThrowStmt`, `TryStmt`, …) and produces a single CFG
//! that contains the union of every callable's intra-procedural flow.
//!
//! # Design choices
//! - One synthetic entry block; one synthetic exit block. All
//!   `ReturnStmt` nodes wire into the exit block. This keeps the
//!   connected-component count `P = 1` so cyclomatic complexity is
//!   `E - N + 2`.
//! - Each callable (`FunctionDecl` / `MethodDecl`) is built independently
//!   and appended; module-level top-level statements are treated as one
//!   additional implicit callable.
//! - Decision nodes (the loop test, the if condition, the switch
//!   discriminant) are *part* of the basic block that ends in the
//!   branch. Branch successors are wired with labeled edges (`True` /
//!   `False` / `SwitchCase`).
//! - `break` / `continue` are resolved against the innermost enclosing
//!   loop or switch via a stack maintained during the walk.
//!
//! This is intentionally a *structural* CFG — it does not unfold
//! short-circuit operators, ternary expressions, or generator
//! expressions. Those would inflate cyclomatic complexity in ways the
//! README's policy doesn't currently price.

use std::collections::HashMap;

use super::models::{BasicBlock, Blocks, CFGEdge, EdgeKind};
use crate::graphs::uast::models::{AttributeValue, UASTNode};

/// Stack frame for break/continue resolution within a loop.
struct LoopContext {
    /// Block id to jump to on `continue`.
    continue_target: usize,
    /// Block id to jump to on `break`.
    break_target: usize,
}

/// Mutable state threaded through the recursive builder.
struct CFGBuildState {
    blocks: Blocks,
    edges: Vec<CFGEdge>,
    next_id: usize,
    loop_stack: Vec<LoopContext>,
    exit_id: usize,
}

impl CFGBuildState {
    fn new() -> Self {
        CFGBuildState {
            blocks: Blocks::new(),
            edges: Vec::new(),
            next_id: 0,
            loop_stack: Vec::new(),
            exit_id: 0,
        }
    }

    fn new_block(&mut self, label: &str) -> usize {
        let id = self.next_id;
        self.blocks.insert(id, BasicBlock::new(id, label));
        self.next_id += 1;
        id
    }

    fn add_edge(&mut self, source: usize, target: usize, kind: EdgeKind) {
        self.edges.push(CFGEdge::new(source, target, kind));
    }

    fn push_statement(&mut self, block_id: usize, stmt: &UASTNode) {
        self.blocks
            .get_mut(&block_id)
            .expect("block_id was returned by new_block earlier in this build")
            .statements
            .push(stmt.clone());
    }
}

/// Build a CFG covering every callable reachable from `uast_root`.
///
/// Returns `(blocks, edges, entry_id, exit_id)`.
///
/// The entry block dispatches by unconditional edges into each callable's
/// individual entry; all callables share the single synthetic exit
/// block. Module-level top-level statements (those not inside any
/// callable) form one additional implicit callable.
pub fn build_cfg_from_uast(uast_root: &UASTNode) -> (Blocks, Vec<CFGEdge>, usize, usize) {
    let mut state = CFGBuildState::new();

    let entry_id = state.new_block("entry");
    let exit_id = state.new_block("exit");
    state.exit_id = exit_id;

    let callables = collect_callables(uast_root);
    if callables.is_empty() {
        // An empty / declaration-only file — wire entry directly to exit
        // so the CFG remains connected and cyclomatic = E - N + 2 = 1.
        state.add_edge(entry_id, exit_id, EdgeKind::Unconditional);
        return (state.blocks, state.edges, entry_id, state.exit_id);
    }

    for callable_node in &callables {
        let callable_entry = state.new_block(&format!("call_{}", callable_node.kind));
        state.add_edge(entry_id, callable_entry, EdgeKind::Unconditional);
        let body = function_body(callable_node);
        if let Some(tail_id) = build_block_sequence(&mut state, &body, callable_entry) {
            state.add_edge(tail_id, state.exit_id, EdgeKind::Unconditional);
        }
    }

    (state.blocks, state.edges, entry_id, state.exit_id)
}

// --- Recursive walker --------------------------------------------------

/// Lay out a straight-line sequence of statements, branching out as
/// needed for control-flow primitives.
///
/// Returns the id of the block that fall-through reaches, or `None` if
/// flow is terminated by a return/break/continue/throw before the end of
/// the sequence.
fn build_block_sequence(
    state: &mut CFGBuildState,
    statements: &[&UASTNode],
    mut current_id: usize,
) -> Option<usize> {
    for stmt in statements {
        match stmt.kind.as_str() {
            "IfStmt" => current_id = build_if(state, stmt, current_id),
            "ForStmt" | "WhileStmt" => current_id = build_loop(state, stmt, current_id),
            "MatchStmt" => current_id = build_match(state, stmt, current_id),
            "TryStmt" => current_id = build_try(state, stmt, current_id),
            "ReturnStmt" => {
                state.push_statement(current_id, stmt);
                state.add_edge(current_id, state.exit_id, EdgeKind::Return);
                return None;
            }
            "ThrowStmt" => {
                state.push_statement(current_id, stmt);
                state.add_edge(current_id, state.exit_id, EdgeKind::Exception);
                return None;
            }
            "BreakStmt" => {
                if let Some(target) = state.loop_stack.last().map(|ctx| ctx.break_target) {
                    state.add_edge(current_id, target, EdgeKind::Break);
                }
                return None;
            }
            "ContinueStmt" => {
                if let Some(target) = state.loop_stack.last().map(|ctx| ctx.continue_target) {
                    state.add_edge(current_id, target, EdgeKind::Continue);
                }
                return None;
            }
            _ => {
                // Recurse into nested blocks/expressions to surface nested
                // decisions (Python `if` inside a list comprehension is
                // *not* surfaced — comprehensions are intentionally not
                // unfolded).
                let inner = children_with_control_flow(stmt);
                if !inner.is_empty() {
                    // Nested callables (arrow fns, object methods) get
                    // their own entry block so an inner `return` does not
                    // terminate the enclosing function's fall-through
                    // block.
                    let nested_entry = state.new_block("nested");
                    state.add_edge(current_id, nested_entry, EdgeKind::Unconditional);
                    if let Some(inner_tail) = build_block_sequence(state, &inner, nested_entry) {
                        current_id = inner_tail;
                    }
                } else {
                    state.push_statement(current_id, stmt);
                }
            }
        }
    }
    Some(current_id)
}

/// Wire `IfStmt` into THEN / ELSE branches with a join block.
fn build_if(state: &mut CFGBuildState, stmt: &UASTNode, current_id: usize) -> usize {
    state.push_statement(current_id, stmt); // records the predicate
    let join_block = state.new_block("if_join");

    let (then_branch, else_branch) = if_branches(stmt);

    let then_block = state.new_block("if_then");
    state.add_edge(current_id, then_block, EdgeKind::True);
    if let Some(then_tail) = build_block_sequence(state, &then_branch, then_block) {
        state.add_edge(then_tail, join_block, EdgeKind::Unconditional);
    }

    if !else_branch.is_empty() {
        let else_block = state.new_block("if_else");
        state.add_edge(current_id, else_block, EdgeKind::False);
        if let Some(else_tail) = build_block_sequence(state, &else_branch, else_block) {
            state.add_edge(else_tail, join_block, EdgeKind::Unconditional);
        }
    } else {
        state.add_edge(current_id, join_block, EdgeKind::False);
    }

    join_block
}

/// Wire `ForStmt` / `WhileStmt`: header → body → back-edge, plus exit.
fn build_loop(state: &mut CFGBuildState, stmt: &UASTNode, current_id: usize) -> usize {
    let header = state.new_block("loop_header");
    state.add_edge(current_id, header, EdgeKind::Unconditional);
    state.push_statement(header, stmt);

    let body_entry = state.new_block("loop_body");
    let after = state.new_block("loop_after");

    state.add_edge(header, body_entry, EdgeKind::True);
    state.add_edge(header, after, EdgeKind::False);

    state.loop_stack.push(LoopContext {
        continue_target: header,
        break_target: after,
    });
    let body = loop_body(stmt);
    let body_tail = build_block_sequence(state, &body, body_entry);
    state.loop_stack.pop();

    if let Some(body_tail) = body_tail {
        state.add_edge(body_tail, header, EdgeKind::Loopback);
    }

    after
}

/// Wire `MatchStmt` as N case branches converging on a join.
fn build_match(state: &mut CFGBuildState, stmt: &UASTNode, current_id: usize) -> usize {
    state.push_statement(current_id, stmt);
    let join_block = state.new_block("match_join");

    let arms = match_arms(stmt);
    if arms.is_empty() {
        state.add_edge(current_id, join_block, EdgeKind::Unconditional);
        return join_block;
    }

    for arm in &arms {
        let arm_block = state.new_block("match_arm");
        state.add_edge(current_id, arm_block, EdgeKind::SwitchCase);
        if let Some(arm_tail) = build_block_sequence(state, std::slice::from_ref(arm), arm_block) {
            state.add_edge(arm_tail, join_block, EdgeKind::Unconditional);
        }
    }

    join_block
}

/// Wire `TryStmt` as try-body with an exception fall-through to each handler.
fn build_try(state: &mut CFGBuildState, stmt: &UASTNode, current_id: usize) -> usize {
    state.push_statement(current_id, stmt);
    let join_block = state.new_block("try_join");

    let body_block = state.new_block("try_body");
    state.add_edge(current_id, body_block, EdgeKind::Unconditional);
    let try_children: Vec<&UASTNode> = stmt
        .children
        .iter()
        .filter(|c| c.kind != "Unknown")
        .collect();
    if let Some(body_tail) = build_block_sequence(state, &try_children, body_block) {
        state.add_edge(body_tail, join_block, EdgeKind::Unconditional);
    }

    // One EXCEPTION edge from current_id directly to join_block represents
    // the implicit "exception unhandled here" path. Fine-grained handler
    // modeling is out of scope for v1.
    state.add_edge(current_id, join_block, EdgeKind::Exception);
    join_block
}

// --- UAST-shape helpers -------------------------------------------------

/// Find every `FunctionDecl` / `MethodDecl` recursively.
///
/// The top-level module body is added as a synthetic callable so
/// module-level control flow gets analyzed too.
///
/// Returns owned nodes (cloned from the tree, or freshly built for the
/// module-level synthetic callable) rather than borrows, so every
/// downstream helper in this module can work uniformly on `&UASTNode`
/// without threading two different lifetimes through the recursive
/// walker.
fn collect_callables(root: &UASTNode) -> Vec<UASTNode> {
    let mut found: Vec<UASTNode> = Vec::new();
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl") {
            found.push(node.clone());
            continue; // don't descend into nested defs — nested counted separately
        }
        stack.extend(node.children.iter().rev());
    }

    if root.kind == "File" {
        // Module-level top: everything not nested inside a callable.
        let module_children: Vec<UASTNode> = root
            .children
            .iter()
            .filter(|c| !matches!(c.kind.as_str(), "FunctionDecl" | "MethodDecl" | "TypeDecl"))
            .cloned()
            .collect();
        if !module_children.is_empty() {
            found.push(UASTNode {
                kind: "FunctionDecl".to_string(),
                lang: root.lang.clone(),
                span: root.span.clone(),
                native: root.native.clone(),
                attributes: HashMap::from([
                    ("synthetic".to_string(), AttributeValue::Bool(true)),
                    (
                        "scope".to_string(),
                        AttributeValue::Str("module".to_string()),
                    ),
                ]),
                children: module_children,
                id: String::new(),
            });
        }
    }

    found
}

const STATEMENT_KINDS: &[&str] = &[
    "IfStmt",
    "ForStmt",
    "WhileStmt",
    "MatchStmt",
    "TryStmt",
    "ReturnStmt",
    "BreakStmt",
    "ContinueStmt",
    "ThrowStmt",
    "ExprStmt",
    "AssignExpr",
    "CallExpr",
    "VarDecl",
];

/// Native tree-sitter node kinds for a switch/match case arm across every
/// supported language. `build_match` emits one CFG branch per arm, so
/// [`match_arms`] returns these nodes verbatim rather than unwrapping into
/// their bodies (which would flatten a multi-statement arm and inflate the
/// branch count).
const ARM_NATIVE_KINDS: &[&str] = &[
    "match_arm",          // Rust
    "case_clause",        // Python `match`
    "expression_case",    // Go `switch`
    "type_case",          // Go type switch
    "default_case",       // Go `default:`
    "communication_case", // Go `select`
    "switch_case",        // JS / TS `case`
    "switch_default",     // JS / TS `default`
    "case_statement",     // C++ `case` / `default`
];

/// Whether `node` is a switch/match case arm (see [`ARM_NATIVE_KINDS`]).
fn is_case_arm(node: &UASTNode) -> bool {
    node.kind == "Unknown" && ARM_NATIVE_KINDS.contains(&node.native.node_kind.as_str())
}

/// Native tree-sitter node kinds that act as transparent block
/// containers (no UAST kind of their own — mapped to `"Unknown"`).
const BLOCK_NATIVE_KINDS: &[&str] = &[
    "block",
    "suite",
    "compound_statement",
    "statement_block",
    "function_body",
    "do_statement",
    "else_clause",
    "elif_clause",
    "statement_list", // Go wraps a `block`'s statements one level deeper
    "expression_case",
    "type_case",
    "default_case",
    "communication_case",
];

fn is_block_container(node: &UASTNode) -> bool {
    node.kind == "Unknown" && BLOCK_NATIVE_KINDS.contains(&node.native.node_kind.as_str())
}

/// Flatten a child list, recursing into block-shaped `Unknown` nodes, to
/// yield the statement-level UAST nodes contained within.
///
/// A "statement" is any UAST node whose `kind` is in
/// [`STATEMENT_KINDS`]. Identifiers, parameters and similar
/// non-statement nodes are skipped.
fn unwrap_to_statements<'a>(nodes: impl IntoIterator<Item = &'a UASTNode>) -> Vec<&'a UASTNode> {
    let mut out = Vec::new();
    for child in nodes {
        if STATEMENT_KINDS.contains(&child.kind.as_str()) {
            out.push(child);
        } else if is_block_container(child) {
            out.extend(unwrap_to_statements(&child.children));
        }
    }
    out
}

/// Statements that constitute the body of a function/method.
fn function_body(callable_node: &UASTNode) -> Vec<&UASTNode> {
    unwrap_to_statements(&callable_node.children)
}

/// Return `(then_statements, else_statements)` for an `IfStmt`.
///
/// The then-block is located by *kind* (the first block-container
/// child), not by a fixed position — the children preceding it vary by
/// grammar and aren't otherwise meaningful here: a bare predicate (most
/// grammars), a predicate plus an init-clause (Go's `if x := f(); cond
/// {}`, C++17's `if (init; cond) {}`), or a predicate with an
/// intervening comment. Everything after the then-block is
/// else-content, one level of unwrapping happening via
/// [`unwrap_to_statements`] either way:
///
/// - Python / JS wrap it in an explicit `else_clause` / `elif_clause`
///   node, whose own children are the actual else statements (or, for
///   `elif`, another predicate + body — multiple `elif` clauses are
///   intentionally flattened into one else-body bucket rather than
///   nested; this is an existing, accepted structural approximation).
/// - Go / C++ / Rust have no such wrapper: the child *is* either a plain
///   block (`else { ... }`) or a nested `if_statement` (`else if`).
fn if_branches(stmt: &UASTNode) -> (Vec<&UASTNode>, Vec<&UASTNode>) {
    let then_idx = stmt.children.iter().position(is_block_container);
    let Some(then_idx) = then_idx else {
        return (Vec::new(), Vec::new());
    };

    let then_body = unwrap_to_statements(std::iter::once(&stmt.children[then_idx]));
    let else_body = unwrap_to_statements(stmt.children[then_idx + 1..].iter());
    (then_body, else_body)
}

/// Extract loop-body statements, skipping test/iterator clauses.
///
/// The body is located by kind (the first block-container child), not
/// by position — a condition-less Go `for { }` has no leading
/// test/clause child at all, so the body is the *first* child rather
/// than "everything after the first child".
fn loop_body(stmt: &UASTNode) -> Vec<&UASTNode> {
    match stmt.children.iter().find(|c| is_block_container(c)) {
        Some(body) => unwrap_to_statements(std::iter::once(body)),
        None => Vec::new(),
    }
}

/// Extract case arms from a `MatchStmt`, one entry per arm.
///
/// Arms live in one of two shapes, both checked here:
/// - **Direct children** of the switch node — Go's `switch`/`select`
///   (both discriminant-carrying and discriminant-less) list their
///   `expression_case`/`default_case`/… arms directly.
/// - **Nested one level** inside a body container — Rust `match_block`,
///   Python `block`, JS `switch_body`, C++ `compound_statement` — whose
///   direct children are the arm nodes. The subject/discriminant and any
///   condition wrapper are skipped because they carry no arms.
///
/// Arm nodes are returned verbatim (never unwrapped into their statement
/// bodies) so `build_match` emits exactly one branch per arm; each arm's
/// own body is walked separately for nested decisions.
fn match_arms(stmt: &UASTNode) -> Vec<&UASTNode> {
    let direct: Vec<&UASTNode> = stmt.children.iter().filter(|c| is_case_arm(c)).collect();
    if !direct.is_empty() {
        return direct;
    }
    for node in &stmt.children {
        if node.kind != "Unknown" {
            continue;
        }
        let nested: Vec<&UASTNode> = node.children.iter().filter(|c| is_case_arm(c)).collect();
        if !nested.is_empty() {
            return nested;
        }
    }
    Vec::new()
}

/// Return child statements whose kind affects control flow.
///
/// Used to recurse into block-ish nodes whose UAST kind is generic
/// (`ExprStmt`, `Unknown`) so that nested decisions inside aren't
/// missed.
fn children_with_control_flow(node: &UASTNode) -> Vec<&UASTNode> {
    const RELEVANT_KINDS: &[&str] = &[
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
        "ReturnStmt",
        "BreakStmt",
        "ContinueStmt",
        "ThrowStmt",
    ];
    let mut found = Vec::new();
    for child in &node.children {
        if RELEVANT_KINDS.contains(&child.kind.as_str()) {
            found.push(child);
        } else {
            found.extend(children_with_control_flow(child));
        }
    }
    found
}
