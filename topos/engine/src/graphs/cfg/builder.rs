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

fn build_block_sequence(
    state: &mut CFGBuildState,
    statements: Vec<&UASTNode>,
    current_id: usize,
) -> Option<usize> {
    enum Task<'a> {
        Sequence {
            statements: Vec<&'a UASTNode>,
            index: usize,
            current_id: usize,
            output: usize,
        },
        ContinueSequence {
            statements: Vec<&'a UASTNode>,
            index: usize,
            statement_output: usize,
            output: usize,
        },
        Statement {
            stmt: &'a UASTNode,
            current_id: usize,
            output: usize,
        },
        FinishNested {
            nested_output: usize,
            fallback_id: usize,
            output: usize,
        },
        ContinueIf {
            then_output: usize,
            else_branch: Vec<&'a UASTNode>,
            current_id: usize,
            join_block: usize,
            output: usize,
        },
        FinishElse {
            else_output: usize,
            join_block: usize,
            output: usize,
        },
        FinishLoop {
            body_output: usize,
            header: usize,
            after: usize,
            output: usize,
        },
        MatchArm {
            arms: Vec<&'a UASTNode>,
            index: usize,
            current_id: usize,
            join_block: usize,
            output: usize,
        },
        ContinueMatchArm {
            arms: Vec<&'a UASTNode>,
            index: usize,
            current_id: usize,
            arm_output: usize,
            join_block: usize,
            output: usize,
        },
        FinishTry {
            body_output: usize,
            current_id: usize,
            join_block: usize,
            output: usize,
        },
    }

    fn new_output(outputs: &mut Vec<Option<usize>>) -> usize {
        outputs.push(None);
        outputs.len() - 1
    }

    let mut outputs = vec![None];
    let mut tasks = vec![Task::Sequence {
        statements,
        index: 0,
        current_id,
        output: 0,
    }];

    while let Some(task) = tasks.pop() {
        match task {
            Task::Sequence {
                statements,
                index,
                current_id,
                output,
            } => {
                let Some(&stmt) = statements.get(index) else {
                    outputs[output] = Some(current_id);
                    continue;
                };
                let statement_output = new_output(&mut outputs);
                tasks.push(Task::ContinueSequence {
                    statements,
                    index: index + 1,
                    statement_output,
                    output,
                });
                tasks.push(Task::Statement {
                    stmt,
                    current_id,
                    output: statement_output,
                });
            }
            Task::ContinueSequence {
                statements,
                index,
                statement_output,
                output,
            } => match outputs[statement_output] {
                Some(current_id) => tasks.push(Task::Sequence {
                    statements,
                    index,
                    current_id,
                    output,
                }),
                None => outputs[output] = None,
            },
            Task::Statement {
                stmt,
                current_id,
                output,
            } => match stmt.kind.as_str() {
                "IfStmt" => {
                    state.push_statement(current_id, stmt);
                    let join_block = state.new_block("if_join");
                    let (then_branch, else_branch) = if_branches(stmt);

                    let then_block = state.new_block("if_then");
                    state.add_edge(current_id, then_block, EdgeKind::True);
                    let then_output = new_output(&mut outputs);
                    tasks.push(Task::ContinueIf {
                        then_output,
                        else_branch,
                        current_id,
                        join_block,
                        output,
                    });
                    tasks.push(Task::Sequence {
                        statements: then_branch,
                        index: 0,
                        current_id: then_block,
                        output: then_output,
                    });
                }
                "ForStmt" | "WhileStmt" => {
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
                    let body_output = new_output(&mut outputs);
                    tasks.push(Task::FinishLoop {
                        body_output,
                        header,
                        after,
                        output,
                    });
                    tasks.push(Task::Sequence {
                        statements: loop_body(stmt),
                        index: 0,
                        current_id: body_entry,
                        output: body_output,
                    });
                }
                "MatchStmt" => {
                    state.push_statement(current_id, stmt);
                    let join_block = state.new_block("match_join");
                    let arms = match_arms(stmt);
                    if arms.is_empty() {
                        state.add_edge(current_id, join_block, EdgeKind::Unconditional);
                        outputs[output] = Some(join_block);
                        continue;
                    }

                    tasks.push(Task::MatchArm {
                        arms,
                        index: 0,
                        current_id,
                        join_block,
                        output,
                    });
                }
                "TryStmt" => {
                    state.push_statement(current_id, stmt);
                    let join_block = state.new_block("try_join");
                    let body_block = state.new_block("try_body");
                    state.add_edge(current_id, body_block, EdgeKind::Unconditional);
                    let body_output = new_output(&mut outputs);
                    tasks.push(Task::FinishTry {
                        body_output,
                        current_id,
                        join_block,
                        output,
                    });
                    tasks.push(Task::Sequence {
                        statements: stmt
                            .children
                            .iter()
                            .filter(|child| child.kind != "Unknown")
                            .collect(),
                        index: 0,
                        current_id: body_block,
                        output: body_output,
                    });
                }
                "ReturnStmt" | "ThrowStmt" | "BreakStmt" | "ContinueStmt" => {
                    handle_terminal_stmt(state, stmt, current_id);
                    outputs[output] = None;
                }
                _ => {
                    let inner = children_with_control_flow(stmt);
                    if inner.is_empty() {
                        state.push_statement(current_id, stmt);
                        outputs[output] = Some(current_id);
                    } else {
                        let nested_entry = state.new_block("nested");
                        state.add_edge(current_id, nested_entry, EdgeKind::Unconditional);
                        let nested_output = new_output(&mut outputs);
                        tasks.push(Task::FinishNested {
                            nested_output,
                            fallback_id: current_id,
                            output,
                        });
                        tasks.push(Task::Sequence {
                            statements: inner,
                            index: 0,
                            current_id: nested_entry,
                            output: nested_output,
                        });
                    }
                }
            },
            Task::FinishNested {
                nested_output,
                fallback_id,
                output,
            } => outputs[output] = Some(outputs[nested_output].unwrap_or(fallback_id)),
            Task::ContinueIf {
                then_output,
                else_branch,
                current_id,
                join_block,
                output,
            } => {
                if let Some(then_tail) = outputs[then_output] {
                    state.add_edge(then_tail, join_block, EdgeKind::Unconditional);
                }
                if else_branch.is_empty() {
                    state.add_edge(current_id, join_block, EdgeKind::False);
                    outputs[output] = Some(join_block);
                } else {
                    let else_block = state.new_block("if_else");
                    state.add_edge(current_id, else_block, EdgeKind::False);
                    let else_output = new_output(&mut outputs);
                    tasks.push(Task::FinishElse {
                        else_output,
                        join_block,
                        output,
                    });
                    tasks.push(Task::Sequence {
                        statements: else_branch,
                        index: 0,
                        current_id: else_block,
                        output: else_output,
                    });
                }
            }
            Task::FinishElse {
                else_output,
                join_block,
                output,
            } => {
                if let Some(else_tail) = outputs[else_output] {
                    state.add_edge(else_tail, join_block, EdgeKind::Unconditional);
                }
                outputs[output] = Some(join_block);
            }
            Task::FinishLoop {
                body_output,
                header,
                after,
                output,
            } => {
                state.loop_stack.pop();
                if let Some(body_tail) = outputs[body_output] {
                    state.add_edge(body_tail, header, EdgeKind::Loopback);
                }
                outputs[output] = Some(after);
            }
            Task::MatchArm {
                arms,
                index,
                current_id,
                join_block,
                output,
            } => {
                let Some(&arm) = arms.get(index) else {
                    outputs[output] = Some(join_block);
                    continue;
                };
                let arm_block = state.new_block("match_arm");
                state.add_edge(current_id, arm_block, EdgeKind::SwitchCase);
                let arm_output = new_output(&mut outputs);
                tasks.push(Task::ContinueMatchArm {
                    arms,
                    index: index + 1,
                    current_id,
                    arm_output,
                    join_block,
                    output,
                });
                tasks.push(Task::Sequence {
                    statements: vec![arm],
                    index: 0,
                    current_id: arm_block,
                    output: arm_output,
                });
            }
            Task::ContinueMatchArm {
                arms,
                index,
                current_id,
                arm_output,
                join_block,
                output,
            } => {
                if let Some(arm_tail) = outputs[arm_output] {
                    state.add_edge(arm_tail, join_block, EdgeKind::Unconditional);
                }
                tasks.push(Task::MatchArm {
                    arms,
                    index,
                    current_id,
                    join_block,
                    output,
                });
            }
            Task::FinishTry {
                body_output,
                current_id,
                join_block,
                output,
            } => {
                if let Some(body_tail) = outputs[body_output] {
                    state.add_edge(body_tail, join_block, EdgeKind::Unconditional);
                }
                state.add_edge(current_id, join_block, EdgeKind::Exception);
                outputs[output] = Some(join_block);
            }
        }
    }

    outputs[0]
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
