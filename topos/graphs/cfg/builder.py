"""
CFG Builder
-----------

Construct a ControlFlowGraph from a UAST root.  The builder walks the
language-independent UAST kind set (``IfStmt``, ``ForStmt``, ``WhileStmt``,
``MatchStmt``, ``ReturnStmt``, ``BreakStmt``, ``ContinueStmt``,
``ThrowStmt``, ``TryStmt``, …) and produces a single CFG that contains the
union of every callable's intra-procedural flow.

Design choices
==============
* One synthetic entry block; one synthetic exit block.  All ``ReturnStmt``
  nodes wire into the exit block.  This keeps the connected-component count
  ``P = 1`` so cyclomatic complexity is ``E - N + 2``.
* Each callable (``FunctionDecl`` / ``MethodDecl``) is built independently
  and appended; module-level top-level statements are treated as one
  additional implicit callable.
* Decision nodes (the loop test, the if condition, the switch
  discriminant) are *part* of the basic block that ends in the branch.
  Branch successors are wired with labeled edges (``TRUE`` / ``FALSE`` /
  ``SWITCH_CASE``).
* ``break`` / ``continue`` are resolved against the innermost enclosing
  loop or switch via a stack maintained during the walk.

This is intentionally a *structural* CFG — it does not unfold short-circuit
operators, ternary expressions, or generator expressions.  Those would
inflate cyclomatic complexity in ways the README's policy doesn't
currently price.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.graphs.cfg.models import BasicBlock, CFGEdge, EdgeKind
from topos.graphs.uast.models import UASTNode


@dataclass
class _LoopContext:
    """Stack frame for break/continue resolution within a loop."""

    continue_target: int  # block id to jump to on `continue`
    break_target: int  # block id to jump to on `break`


_DECISION_KINDS = frozenset({"IfStmt", "ForStmt", "WhileStmt", "MatchStmt"})
_BRANCH_KINDS = frozenset({"BreakStmt", "ContinueStmt", "ReturnStmt", "ThrowStmt"})


@dataclass
class CFGBuildState:
    """Mutable state threaded through the recursive builder."""

    blocks: dict[int, BasicBlock] = field(default_factory=dict)
    edges: list[CFGEdge] = field(default_factory=list)
    _next_id: int = 0
    loop_stack: list[_LoopContext] = field(default_factory=list)
    exit_id: int = -1  # set when exit block is allocated

    def new_block(self, label: str = "") -> BasicBlock:
        block = BasicBlock(id=self._next_id, label=label)
        self.blocks[block.id] = block
        self._next_id += 1
        return block

    def add_edge(
        self, source: int, target: int, kind: EdgeKind = EdgeKind.UNCONDITIONAL
    ) -> None:
        self.edges.append(CFGEdge(source=source, target=target, kind=kind))


def build_cfg_from_uast(
    uast_root: UASTNode,
) -> tuple[dict[int, BasicBlock], list[CFGEdge], int, int]:
    """
    Build a CFG covering every callable reachable from ``uast_root``.

    Returns:
        ``(blocks, edges, entry_id, exit_id)``.

    The entry block dispatches by unconditional edges into each callable's
    individual entry; all callables share the single synthetic exit block.
    Module-level top-level statements (those not inside any callable) form
    one additional implicit callable.
    """
    state = CFGBuildState()

    entry = state.new_block("entry")
    exit_block = state.new_block("exit")
    state.exit_id = exit_block.id

    callables = _collect_callables(uast_root)
    if not callables:
        # An empty / declaration-only file — wire entry directly to exit so
        # the CFG remains connected and cyclomatic = E - N + 2 = 1.
        state.add_edge(entry.id, exit_block.id)
        return state.blocks, state.edges, entry.id, state.exit_id

    for callable_node in callables:
        callable_entry = state.new_block(f"call_{callable_node.kind}")
        state.add_edge(entry.id, callable_entry.id)
        body = _function_body(callable_node)
        tail_id = _build_block_sequence(state, body, callable_entry.id)
        if tail_id is not None:
            state.add_edge(tail_id, state.exit_id)

    return state.blocks, state.edges, entry.id, state.exit_id


# ---------------------------------------------------------------------------
# Recursive walker
# ---------------------------------------------------------------------------


def _build_block_sequence(
    state: CFGBuildState, statements: list[UASTNode], current_id: int
) -> int | None:
    """
    Lay out a straight-line sequence of statements, branching out as
    needed for control-flow primitives.

    Returns the id of the block that fall-through reaches, or ``None`` if
    flow is terminated by a return/break/continue/throw before the end of
    the sequence.
    """
    for stmt in statements:
        if current_id is None:
            return None

        if stmt.kind == "IfStmt":
            current_id = _build_if(state, stmt, current_id)
        elif stmt.kind in {"ForStmt", "WhileStmt"}:
            current_id = _build_loop(state, stmt, current_id)
        elif stmt.kind == "MatchStmt":
            current_id = _build_match(state, stmt, current_id)
        elif stmt.kind == "TryStmt":
            current_id = _build_try(state, stmt, current_id)
        elif stmt.kind == "ReturnStmt":
            state.blocks[current_id].statements.append(stmt)
            state.add_edge(current_id, state.exit_id, EdgeKind.RETURN)
            return None
        elif stmt.kind == "ThrowStmt":
            state.blocks[current_id].statements.append(stmt)
            state.add_edge(current_id, state.exit_id, EdgeKind.EXCEPTION)
            return None
        elif stmt.kind == "BreakStmt":
            if state.loop_stack:
                state.add_edge(
                    current_id, state.loop_stack[-1].break_target, EdgeKind.BREAK
                )
            return None
        elif stmt.kind == "ContinueStmt":
            if state.loop_stack:
                state.add_edge(
                    current_id,
                    state.loop_stack[-1].continue_target,
                    EdgeKind.CONTINUE,
                )
            return None
        else:
            # Recurse into nested blocks/expressions to surface nested
            # decisions (Python `if` inside a list comprehension is *not*
            # surfaced — comprehensions are intentionally not unfolded).
            inner = _children_with_control_flow(stmt)
            if inner:
                # Nested callables (arrow fns, object methods) get their own
                # entry block so an inner `return` does not terminate the enclosing
                # function's fall-through block.
                nested_entry = state.new_block("nested")
                state.add_edge(current_id, nested_entry.id)
                inner_tail = _build_block_sequence(state, inner, nested_entry.id)
                if inner_tail is not None:
                    current_id = inner_tail
            else:
                state.blocks[current_id].statements.append(stmt)

    return current_id


def _build_if(state: CFGBuildState, stmt: UASTNode, current_id: int) -> int:
    """Wire ``IfStmt`` into THEN / ELSE branches with a join block."""
    state.blocks[current_id].statements.append(stmt)  # records the predicate
    join_block = state.new_block("if_join")

    then_branch, else_branch = _if_branches(stmt)

    then_block = state.new_block("if_then")
    state.add_edge(current_id, then_block.id, EdgeKind.TRUE)
    then_tail = _build_block_sequence(state, then_branch, then_block.id)
    if then_tail is not None:
        state.add_edge(then_tail, join_block.id)

    if else_branch:
        else_block = state.new_block("if_else")
        state.add_edge(current_id, else_block.id, EdgeKind.FALSE)
        else_tail = _build_block_sequence(state, else_branch, else_block.id)
        if else_tail is not None:
            state.add_edge(else_tail, join_block.id)
    else:
        state.add_edge(current_id, join_block.id, EdgeKind.FALSE)

    return join_block.id


def _build_loop(state: CFGBuildState, stmt: UASTNode, current_id: int) -> int:
    """Wire ``ForStmt`` / ``WhileStmt``: header → body → back-edge, plus exit."""
    header = state.new_block("loop_header")
    state.add_edge(current_id, header.id)
    header.statements.append(stmt)

    body_entry = state.new_block("loop_body")
    after = state.new_block("loop_after")

    state.add_edge(header.id, body_entry.id, EdgeKind.TRUE)
    state.add_edge(header.id, after.id, EdgeKind.FALSE)

    state.loop_stack.append(
        _LoopContext(continue_target=header.id, break_target=after.id)
    )
    body_tail = _build_block_sequence(state, _loop_body(stmt), body_entry.id)
    state.loop_stack.pop()

    if body_tail is not None:
        state.add_edge(body_tail, header.id, EdgeKind.LOOP_BACK)

    return after.id


def _build_match(state: CFGBuildState, stmt: UASTNode, current_id: int) -> int:
    """Wire ``MatchStmt`` as N case branches converging on a join."""
    state.blocks[current_id].statements.append(stmt)
    join_block = state.new_block("match_join")

    arms = _match_arms(stmt)
    if not arms:
        state.add_edge(current_id, join_block.id, EdgeKind.UNCONDITIONAL)
        return join_block.id

    for arm in arms:
        arm_block = state.new_block("match_arm")
        state.add_edge(current_id, arm_block.id, EdgeKind.SWITCH_CASE)
        arm_tail = _build_block_sequence(state, [arm], arm_block.id)
        if arm_tail is not None:
            state.add_edge(arm_tail, join_block.id)

    return join_block.id


def _build_try(state: CFGBuildState, stmt: UASTNode, current_id: int) -> int:
    """Wire ``TryStmt`` as try-body with an exception fall-through to each handler."""
    state.blocks[current_id].statements.append(stmt)
    join_block = state.new_block("try_join")

    body_block = state.new_block("try_body")
    state.add_edge(current_id, body_block.id)
    try_children = [c for c in stmt.children if c.kind != "Unknown"]
    body_tail = _build_block_sequence(state, try_children, body_block.id)
    if body_tail is not None:
        state.add_edge(body_tail, join_block.id)

    # One EXCEPTION edge from current_id directly to join_block represents
    # the implicit "exception unhandled here" path.  Fine-grained handler
    # modeling is out of scope for v1.
    state.add_edge(current_id, join_block.id, EdgeKind.EXCEPTION)
    return join_block.id


# ---------------------------------------------------------------------------
# UAST-shape helpers
# ---------------------------------------------------------------------------


def _collect_callables(root: UASTNode) -> list[UASTNode]:
    """Find every FunctionDecl / MethodDecl recursively.

    The top-level module body is added as a synthetic callable so
    module-level control flow gets analyzed too.
    """
    found: list[UASTNode] = []
    stack: list[UASTNode] = [root]
    while stack:
        node = stack.pop()
        if node.kind in {"FunctionDecl", "MethodDecl"}:
            found.append(node)
            continue  # don't descend into nested defs — nested counted separately
        stack.extend(reversed(node.children))

    if root.kind == "File":
        # Module-level top: everything not nested inside a callable.
        module_body_pseudo = UASTNode(
            kind="FunctionDecl",
            lang=root.lang,
            span=root.span,
            native=root.native,
            attributes={"synthetic": True, "scope": "module"},
            children=[
                c
                for c in root.children
                if c.kind not in {"FunctionDecl", "MethodDecl", "TypeDecl"}
            ],
        )
        if module_body_pseudo.children:
            found.append(module_body_pseudo)

    return found


_STATEMENT_KINDS = frozenset(
    {
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
    }
)

# Native tree-sitter node kinds that act as transparent block containers
# (no UAST kind of their own — mapped to "Unknown").
_BLOCK_NATIVE_KINDS = frozenset(
    {
        "block",
        "suite",
        "compound_statement",
        "statement_block",
        "function_body",
        "do_statement",
        "else_clause",
        "elif_clause",
        "statement_list",  # Go wraps a `block`'s statements one level deeper
        "expression_case",  # Go `switch` arm (tagged or tagless)
        "type_case",  # Go `switch x.(type)` arm
        "default_case",  # Go `switch`/`select` default arm
        "communication_case",  # Go `select` arm
    }
)


def _is_block_container(node: UASTNode) -> bool:
    return node.kind == "Unknown" and node.native.node_kind in _BLOCK_NATIVE_KINDS


def _unwrap_to_statements(nodes: list[UASTNode]) -> list[UASTNode]:
    """
    Flatten a child list, recursing into block-shaped Unknown nodes, to
    yield the statement-level UAST nodes contained within.

    A "statement" is any UAST node whose ``kind`` is in
    ``_STATEMENT_KINDS``.  Identifiers, parameters and similar
    non-statement nodes are skipped.
    """
    out: list[UASTNode] = []
    for child in nodes:
        if child.kind in _STATEMENT_KINDS:
            out.append(child)
        elif _is_block_container(child):
            out.extend(_unwrap_to_statements(child.children))
    return out


def _function_body(callable_node: UASTNode) -> list[UASTNode]:
    """Statements that constitute the body of a function/method."""
    return _unwrap_to_statements(callable_node.children)


def _if_branches(stmt: UASTNode) -> tuple[list[UASTNode], list[UASTNode]]:
    """Return ``(then_statements, else_statements)`` for an IfStmt.

    The then-block is located by *kind* (the first block-container child),
    not by a fixed position — the children preceding it vary by grammar and
    aren't otherwise meaningful here: a bare predicate (most grammars), a
    predicate plus an init-clause (Go's ``if x := f(); cond {}``, C++17's
    ``if (init; cond) {}``), or a predicate with an intervening comment.
    Everything after the then-block is else-content, one level of
    unwrapping happening via ``_unwrap_to_statements`` either way:

    * Python / JS wrap it in an explicit ``else_clause`` / ``elif_clause``
      node, whose own children are the actual else statements (or, for
      ``elif``, another predicate + body — multiple ``elif`` clauses are
      intentionally flattened into one ``else_body`` bucket rather than
      nested; this is an existing, accepted structural approximation).
    * Go / C++ / Rust have no such wrapper: the child *is* either a plain
      block (``else { ... }``) or a nested ``if_statement`` (``else if``).
    """
    children = list(stmt.children)
    then_idx = next(
        (i for i, child in enumerate(children) if _is_block_container(child)), None
    )
    if then_idx is None:
        return [], []

    then_body = _unwrap_to_statements([children[then_idx]])
    else_body = _unwrap_to_statements(children[then_idx + 1 :])
    return then_body, else_body


def _loop_body(stmt: UASTNode) -> list[UASTNode]:
    """Extract loop-body statements, skipping test/iterator clauses."""
    if not stmt.children:
        return []
    # The first child is the loop test / iterator binding.  Everything
    # after is body — but the body is wrapped in a block.
    return _unwrap_to_statements(stmt.children[1:])


_CASE_ARM_NATIVE_KINDS = frozenset(
    {"expression_case", "type_case", "default_case", "communication_case"}
)


def _match_arms(stmt: UASTNode) -> list[UASTNode]:
    """Extract case arms from a MatchStmt.

    Usually the first child is the discriminant/subject (Python ``match``,
    Rust ``match``, Go's tagged ``switch``/``type switch``) and is skipped.
    Go also has discriminant-less forms (``switch { case ... }``,
    ``select { case ... }``) where the first child is itself a case arm —
    detected via its native node kind so every arm is kept.
    """
    if not stmt.children:
        return []
    children = list(stmt.children)
    first = children[0]
    if first.kind == "Unknown" and first.native.node_kind in _CASE_ARM_NATIVE_KINDS:
        return _unwrap_to_statements(children)
    return _unwrap_to_statements(children[1:])


def _children_with_control_flow(node: UASTNode) -> list[UASTNode]:
    """
    Return child statements whose kind affects control flow.

    Used to recurse into block-ish nodes whose UAST kind is generic
    (``ExprStmt``, ``Unknown``) so that nested decisions inside aren't
    missed.
    """
    relevant_kinds = {
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
        "ReturnStmt",
        "BreakStmt",
        "ContinueStmt",
        "ThrowStmt",
    }
    found: list[UASTNode] = []
    for child in node.children:
        if child.kind in relevant_kinds:
            found.append(child)
        else:
            found.extend(_children_with_control_flow(child))
    return found
