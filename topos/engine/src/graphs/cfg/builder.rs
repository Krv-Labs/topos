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

use super::models::{BasicBlock, Blocks, CFGEdge, EdgeKind};
use crate::graphs::uast::models::UASTNode;

/// Stack frame for break/continue resolution within a loop.
struct LoopContext {
    /// Block id to jump to on `continue`.
    continue_target: usize,
    /// Block id to jump to on `break`.
    break_target: usize,
}

/// Mutable state threaded through the iterative builder.
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
        let mut owned = stmt.clone();
        if owned.id.is_empty() {
            owned.id = anonymous_node_key(stmt);
        }
        self.blocks
            .get_mut(&block_id)
            .expect("block_id was returned by new_block earlier in this build")
            .statements
            .push(owned);
    }
}

fn anonymous_node_key(node: &UASTNode) -> String {
    format!("anon::{:x}", std::ptr::from_ref(node) as usize)
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

    let callables = collect_callable_bodies(uast_root);
    if callables.is_empty() {
        // An empty / declaration-only file — wire entry directly to exit
        // so the CFG remains connected and cyclomatic = E - N + 2 = 1.
        state.add_edge(entry_id, exit_id, EdgeKind::Unconditional);
        return (state.blocks, state.edges, entry_id, state.exit_id);
    }

    for callable in callables {
        let callable_entry = state.new_block(&format!("call_{}", callable.label));
        state.add_edge(entry_id, callable_entry, EdgeKind::Unconditional);
        if let Some(tail_id) = build_block_sequence(&mut state, callable.statements, callable_entry)
        {
            state.add_edge(tail_id, state.exit_id, EdgeKind::Unconditional);
        }
    }

    (state.blocks, state.edges, entry_id, state.exit_id)
}

// --- Iterative walker --------------------------------------------------

/// Lay out a straight-line sequence of statements, branching out as
/// needed for control-flow primitives.
///
/// Returns the id of the block that fall-through reaches, or `None` if
/// flow is terminated by a return/break/continue/throw before the end of
/// the sequence.
fn handle_terminal_stmt(state: &mut CFGBuildState, stmt: &UASTNode, current_id: usize) -> bool {
    match stmt.kind.as_str() {
        "ReturnStmt" => {
            state.push_statement(current_id, stmt);
            state.add_edge(current_id, state.exit_id, EdgeKind::Return);
            true
        }
        "ThrowStmt" => {
            state.push_statement(current_id, stmt);
            state.add_edge(current_id, state.exit_id, EdgeKind::Exception);
            true
        }
        "BreakStmt" => {
            if let Some(target) = state.loop_stack.last().map(|ctx| ctx.break_target) {
                state.add_edge(current_id, target, EdgeKind::Break);
            }
            true
        }
        "ContinueStmt" => {
            if let Some(target) = state.loop_stack.last().map(|ctx| ctx.continue_target) {
                state.add_edge(current_id, target, EdgeKind::Continue);
            }
            true
        }
        _ => false,
    }
}

enum BuildTask<'a> {
    Sequence(SequenceTask<'a>),
    Statement {
        stmt: &'a UASTNode,
        current_id: usize,
        output: usize,
    },
    Continuation(ContinuationTask<'a>),
}

enum SequenceTask<'a> {
    Next {
        statements: Vec<&'a UASTNode>,
        index: usize,
        current_id: usize,
        output: usize,
    },
    Resume {
        statements: Vec<&'a UASTNode>,
        index: usize,
        statement_output: usize,
        output: usize,
    },
}

struct MatchCursor<'a> {
    arms: Vec<&'a UASTNode>,
    index: usize,
    current_id: usize,
    join_block: usize,
    output: usize,
}

enum ContinuationTask<'a> {
    Nested {
        nested_output: usize,
        fallback_id: usize,
        output: usize,
    },
    If {
        then_output: usize,
        else_branch: Vec<&'a UASTNode>,
        current_id: usize,
        join_block: usize,
        output: usize,
    },
    Else {
        else_output: usize,
        join_block: usize,
        output: usize,
    },
    Loop {
        body_output: usize,
        header: usize,
        after: usize,
        output: usize,
    },
    MatchArm(MatchCursor<'a>),
    MatchArmDone {
        cursor: MatchCursor<'a>,
        arm_output: usize,
    },
    Try {
        body_output: usize,
        current_id: usize,
        join_block: usize,
        output: usize,
    },
}

struct BuildMachine<'state, 'a> {
    state: &'state mut CFGBuildState,
    outputs: Vec<Option<usize>>,
    tasks: Vec<BuildTask<'a>>,
}

impl<'state, 'a> BuildMachine<'state, 'a> {
    fn new(state: &'state mut CFGBuildState) -> Self {
        Self {
            state,
            outputs: vec![None],
            tasks: Vec::new(),
        }
    }

    fn new_output(&mut self) -> usize {
        self.outputs.push(None);
        self.outputs.len() - 1
    }

    fn push_sequence(
        &mut self,
        statements: Vec<&'a UASTNode>,
        index: usize,
        current_id: usize,
        output: usize,
    ) {
        self.tasks.push(BuildTask::Sequence(SequenceTask::Next {
            statements,
            index,
            current_id,
            output,
        }));
    }

    fn run(&mut self) {
        while let Some(task) = self.tasks.pop() {
            self.execute(task);
        }
    }

    fn execute(&mut self, task: BuildTask<'a>) {
        match task {
            BuildTask::Sequence(task) => self.execute_sequence(task),
            BuildTask::Statement {
                stmt,
                current_id,
                output,
            } => self.execute_statement(stmt, current_id, output),
            BuildTask::Continuation(task) => self.execute_continuation(task),
        }
    }

    fn execute_sequence(&mut self, task: SequenceTask<'a>) {
        match task {
            SequenceTask::Next {
                statements,
                index,
                current_id,
                output,
            } => self.next_statement(statements, index, current_id, output),
            SequenceTask::Resume {
                statements,
                index,
                statement_output,
                output,
            } => self.resume_sequence(statements, index, statement_output, output),
        }
    }

    fn next_statement(
        &mut self,
        statements: Vec<&'a UASTNode>,
        index: usize,
        current_id: usize,
        output: usize,
    ) {
        let Some(&stmt) = statements.get(index) else {
            self.outputs[output] = Some(current_id);
            return;
        };
        let statement_output = self.new_output();
        self.tasks.push(BuildTask::Sequence(SequenceTask::Resume {
            statements,
            index: index + 1,
            statement_output,
            output,
        }));
        self.tasks.push(BuildTask::Statement {
            stmt,
            current_id,
            output: statement_output,
        });
    }

    fn resume_sequence(
        &mut self,
        statements: Vec<&'a UASTNode>,
        index: usize,
        statement_output: usize,
        output: usize,
    ) {
        match self.outputs[statement_output] {
            Some(current_id) => self.push_sequence(statements, index, current_id, output),
            None => self.outputs[output] = None,
        }
    }

    fn execute_statement(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        match stmt.kind.as_str() {
            "IfStmt" => self.start_if(stmt, current_id, output),
            "ForStmt" | "WhileStmt" => self.start_loop(stmt, current_id, output),
            "MatchStmt" => self.start_match(stmt, current_id, output),
            "TryStmt" => self.start_try(stmt, current_id, output),
            "ReturnStmt" | "ThrowStmt" | "BreakStmt" | "ContinueStmt" => {
                self.finish_terminal(stmt, current_id, output)
            }
            _ => self.start_nested(stmt, current_id, output),
        }
    }

    fn start_if(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        self.state.push_statement(current_id, stmt);
        let join_block = self.state.new_block("if_join");
        let (then_branch, else_branch) = if_branches(stmt);
        let then_block = self.state.new_block("if_then");
        self.state.add_edge(current_id, then_block, EdgeKind::True);
        let then_output = self.new_output();
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::If {
                then_output,
                else_branch,
                current_id,
                join_block,
                output,
            }));
        self.push_sequence(then_branch, 0, then_block, then_output);
    }

    fn start_loop(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        let header = self.state.new_block("loop_header");
        self.state
            .add_edge(current_id, header, EdgeKind::Unconditional);
        self.state.push_statement(header, stmt);
        let body_entry = self.state.new_block("loop_body");
        let after = self.state.new_block("loop_after");
        self.state.add_edge(header, body_entry, EdgeKind::True);
        self.state.add_edge(header, after, EdgeKind::False);
        self.state.loop_stack.push(LoopContext {
            continue_target: header,
            break_target: after,
        });
        let body_output = self.new_output();
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::Loop {
                body_output,
                header,
                after,
                output,
            }));
        self.push_sequence(loop_body(stmt), 0, body_entry, body_output);
    }

    fn start_match(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        self.state.push_statement(current_id, stmt);
        let join_block = self.state.new_block("match_join");
        let arms = match_arms(stmt);
        if arms.is_empty() {
            self.state
                .add_edge(current_id, join_block, EdgeKind::Unconditional);
            self.outputs[output] = Some(join_block);
            return;
        }
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::MatchArm(
                MatchCursor {
                    arms,
                    index: 0,
                    current_id,
                    join_block,
                    output,
                },
            )));
    }

    fn start_try(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        self.state.push_statement(current_id, stmt);
        let join_block = self.state.new_block("try_join");
        let body_block = self.state.new_block("try_body");
        self.state
            .add_edge(current_id, body_block, EdgeKind::Unconditional);
        let body_output = self.new_output();
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::Try {
                body_output,
                current_id,
                join_block,
                output,
            }));
        let body = stmt
            .children
            .iter()
            .filter(|child| child.kind != "Unknown")
            .collect();
        self.push_sequence(body, 0, body_block, body_output);
    }

    fn finish_terminal(&mut self, stmt: &UASTNode, current_id: usize, output: usize) {
        handle_terminal_stmt(self.state, stmt, current_id);
        self.outputs[output] = None;
    }

    fn start_nested(&mut self, stmt: &'a UASTNode, current_id: usize, output: usize) {
        let inner = children_with_control_flow(stmt);
        if inner.is_empty() {
            self.state.push_statement(current_id, stmt);
            self.outputs[output] = Some(current_id);
            return;
        }
        let nested_entry = self.state.new_block("nested");
        self.state
            .add_edge(current_id, nested_entry, EdgeKind::Unconditional);
        let nested_output = self.new_output();
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::Nested {
                nested_output,
                fallback_id: current_id,
                output,
            }));
        self.push_sequence(inner, 0, nested_entry, nested_output);
    }

    fn execute_continuation(&mut self, task: ContinuationTask<'a>) {
        match task {
            ContinuationTask::Nested {
                nested_output,
                fallback_id,
                output,
            } => self.finish_nested(nested_output, fallback_id, output),
            ContinuationTask::If {
                then_output,
                else_branch,
                current_id,
                join_block,
                output,
            } => self.finish_if(then_output, else_branch, current_id, join_block, output),
            ContinuationTask::Else {
                else_output,
                join_block,
                output,
            } => {
                if let Some(else_tail) = self.outputs[else_output] {
                    self.state
                        .add_edge(else_tail, join_block, EdgeKind::Unconditional);
                }
                self.outputs[output] = Some(join_block);
            }
            ContinuationTask::Loop {
                body_output,
                header,
                after,
                output,
            } => self.finish_loop(body_output, header, after, output),
            ContinuationTask::MatchArm(cursor) => self.start_match_arm(cursor),
            ContinuationTask::MatchArmDone { cursor, arm_output } => {
                self.finish_match_arm(cursor, arm_output)
            }
            ContinuationTask::Try {
                body_output,
                current_id,
                join_block,
                output,
            } => self.finish_try(body_output, current_id, join_block, output),
        }
    }

    fn finish_nested(&mut self, nested_output: usize, fallback_id: usize, output: usize) {
        self.outputs[output] = Some(self.outputs[nested_output].unwrap_or(fallback_id));
    }

    fn finish_if(
        &mut self,
        then_output: usize,
        else_branch: Vec<&'a UASTNode>,
        current_id: usize,
        join_block: usize,
        output: usize,
    ) {
        if let Some(then_tail) = self.outputs[then_output] {
            self.state
                .add_edge(then_tail, join_block, EdgeKind::Unconditional);
        }
        if else_branch.is_empty() {
            self.state.add_edge(current_id, join_block, EdgeKind::False);
            self.outputs[output] = Some(join_block);
            return;
        }
        let else_block = self.state.new_block("if_else");
        self.state.add_edge(current_id, else_block, EdgeKind::False);
        let else_output = self.new_output();
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::Else {
                else_output,
                join_block,
                output,
            }));
        self.push_sequence(else_branch, 0, else_block, else_output);
    }

    fn finish_loop(&mut self, body_output: usize, header: usize, after: usize, output: usize) {
        self.state.loop_stack.pop();
        if let Some(body_tail) = self.outputs[body_output] {
            self.state.add_edge(body_tail, header, EdgeKind::Loopback);
        }
        self.outputs[output] = Some(after);
    }

    fn start_match_arm(&mut self, cursor: MatchCursor<'a>) {
        let Some(&arm) = cursor.arms.get(cursor.index) else {
            self.outputs[cursor.output] = Some(cursor.join_block);
            return;
        };
        let arm_block = self.state.new_block("match_arm");
        self.state
            .add_edge(cursor.current_id, arm_block, EdgeKind::SwitchCase);
        let arm_output = self.new_output();
        let next = MatchCursor {
            index: cursor.index + 1,
            ..cursor
        };
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::MatchArmDone {
                cursor: next,
                arm_output,
            }));
        self.push_sequence(vec![arm], 0, arm_block, arm_output);
    }

    fn finish_match_arm(&mut self, cursor: MatchCursor<'a>, arm_output: usize) {
        if let Some(arm_tail) = self.outputs[arm_output] {
            self.state
                .add_edge(arm_tail, cursor.join_block, EdgeKind::Unconditional);
        }
        self.tasks
            .push(BuildTask::Continuation(ContinuationTask::MatchArm(cursor)));
    }

    fn finish_try(
        &mut self,
        body_output: usize,
        current_id: usize,
        join_block: usize,
        output: usize,
    ) {
        if let Some(body_tail) = self.outputs[body_output] {
            self.state
                .add_edge(body_tail, join_block, EdgeKind::Unconditional);
        }
        self.state
            .add_edge(current_id, join_block, EdgeKind::Exception);
        self.outputs[output] = Some(join_block);
    }
}

fn build_block_sequence(
    state: &mut CFGBuildState,
    statements: Vec<&UASTNode>,
    current_id: usize,
) -> Option<usize> {
    let mut machine = BuildMachine::new(state);
    machine.push_sequence(statements, 0, current_id, 0);
    machine.run();
    machine.outputs[0]
}
// --- UAST-shape helpers -------------------------------------------------

/// Find every `FunctionDecl` / `MethodDecl` recursively.
///
/// The top-level module body is added as an implicit callable so
/// module-level control flow gets analyzed too. Bodies borrow the original
/// UAST nodes; CFG blocks clone statements only after preserving any
/// anonymous node's original key.
struct CallableBody<'a> {
    label: String,
    statements: Vec<&'a UASTNode>,
}

fn collect_callable_bodies(root: &UASTNode) -> Vec<CallableBody<'_>> {
    let mut found = Vec::new();
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl") {
            found.push(CallableBody {
                label: node.kind.clone(),
                statements: function_body(node),
            });
            continue; // don't descend into nested defs — nested counted separately
        }
        stack.extend(node.children.iter().rev());
    }

    if root.kind == "File" {
        // Module-level top: everything not nested inside a callable.
        let module_children = root
            .children
            .iter()
            .filter(|c| !matches!(c.kind.as_str(), "FunctionDecl" | "MethodDecl" | "TypeDecl"))
            .collect::<Vec<_>>();
        if !module_children.is_empty() {
            found.push(CallableBody {
                label: "FunctionDecl".to_string(),
                statements: unwrap_to_statements(module_children),
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

/// Flatten a child list, descending into block-shaped `Unknown` nodes, to
/// yield the statement-level UAST nodes contained within.
///
/// A "statement" is any UAST node whose `kind` is in
/// [`STATEMENT_KINDS`]. Identifiers, parameters and similar
/// non-statement nodes are skipped.
fn unwrap_to_statements<'a>(nodes: impl IntoIterator<Item = &'a UASTNode>) -> Vec<&'a UASTNode> {
    let mut out = Vec::new();
    let mut stack: Vec<&UASTNode> = nodes.into_iter().collect();
    stack.reverse();
    while let Some(child) = stack.pop() {
        if STATEMENT_KINDS.contains(&child.kind.as_str()) {
            out.push(child);
        } else if is_block_container(child) {
            stack.extend(child.children.iter().rev());
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

/// Number of case arms in a `MatchStmt`. Shared with the AST complexity
/// probe (`ast.max_function_complexity`) so it counts arms the same way the
/// CFG does — a k-way switch contributes k branches.
pub(crate) fn match_arm_count(stmt: &UASTNode) -> usize {
    match_arms(stmt).len()
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
    let mut stack: Vec<&UASTNode> = node.children.iter().rev().collect();
    while let Some(child) = stack.pop() {
        if RELEVANT_KINDS.contains(&child.kind.as_str()) {
            found.push(child);
        } else {
            stack.extend(child.children.iter().rev());
        }
    }
    found
}
