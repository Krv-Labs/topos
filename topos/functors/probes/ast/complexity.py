"""
Per-Function Complexity Analysis
--------------------------------

Provides a function-level breakdown of complexity using the AST.
This is separate from the CFG-based module-level cyclomatic complexity used
in the main program evaluation (SIMPLE generator).

Every ``ProgramObject`` produced by a real parse (``ProgramMorphism`` /
``parse_source``) carries a populated, language-neutral ``uast_root``
(see ``topos.graphs.ast.providers.tree_sitter_provider``), so the primary
implementation below walks that UAST directly to find ``FunctionDecl`` /
``MethodDecl`` nodes -- this works uniformly across every language with a
UAST mapper (Python, Rust, Go, JavaScript, TypeScript, C++), unlike the
previous approach of re-querying Python-specific tree-sitter native node
type strings (``function_definition``/``async_function_definition``),
which silently found zero functions for every other language.

A ``uast_root``-less native tree-sitter fallback is retained for callers
that construct a ``ProgramObject`` directly without populating
``uast_root`` (e.g. lightweight test fixtures); it remains Python-only,
same as before this fix.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

from topos.core.object import ProgramObject

if TYPE_CHECKING:
    from topos.graphs.uast.models import UASTNode

_FUNCTION_NODE_TYPES = ("function_definition", "async_function_definition")
_SCOPE_NODE_TYPES = (*_FUNCTION_NODE_TYPES, "class_definition")

DECISION_NODE_TYPES = frozenset(
    {
        "if_statement",
        "elif_clause",
        "for_statement",
        "while_statement",
        "except_clause",
        "with_statement",
        "assert_statement",
        "conditional_expression",  # ternary
        "boolean_operator",  # and/or short-circuit
        "match_statement",  # Python 3.10+
        "case_clause",
        "list_comprehension",
        "dictionary_comprehension",
        "set_comprehension",
        "generator_expression",
    }
)

DECISION_UAST_KINDS = frozenset(
    {
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
    }
)

# UAST kinds that every language mapper agrees denote a callable.
_FUNCTION_UAST_KINDS = frozenset({"FunctionDecl", "MethodDecl"})

# UAST kinds that introduce a new "scope" for qualified-name/kind purposes
# (mirrors ``_SCOPE_NODE_TYPES`` for the native tree-sitter fallback).
_SCOPE_UAST_KINDS = frozenset({*_FUNCTION_UAST_KINDS, "TypeDecl"})

# tree-sitter grammars use different native node kinds for a declaration's
# name token depending on syntactic position (e.g. tree-sitter-javascript
# emits ``property_identifier`` for a class method's name, not
# ``identifier``; tree-sitter-go emits ``field_identifier``/
# ``type_identifier`` elsewhere). ``UASTNode`` has no stored ``name``
# attribute, so recovering it means scanning a declaration's own children
# for one of these native kinds and slicing the corresponding byte span out
# of the source -- the *native* provenance kind is matched here rather than
# the normalized UAST ``kind``, since none of the mappers classify these
# tokens as ``"Identifier"`` today.
_NAME_NATIVE_KINDS = frozenset(
    {
        "identifier",
        "property_identifier",
        "field_identifier",
        "type_identifier",
        "package_identifier",
    }
)

_ASYNC_PREFIX_RE = re.compile(rb"\Aasync\b")


def _calculate_node_complexity(ast: ProgramObject) -> int:
    """Internal helper to calculate complexity of a specific node/sub-tree."""
    if ast.uast_root is not None:
        return _calculate_cyclomatic_complexity_uast(ast.uast_root)

    decision_count = 0

    for node in ast.traverse():
        if node.type in DECISION_NODE_TYPES:
            decision_count += 1
            if node.type == "boolean_operator":
                decision_count += _count_boolean_operators(node) - 1

    return decision_count + 1


def _calculate_cyclomatic_complexity_uast(root: UASTNode) -> int:
    decision_count = 0

    def walk(node) -> None:
        nonlocal decision_count
        kind = getattr(node, "kind", "")
        if kind in DECISION_UAST_KINDS:
            decision_count += 1
        if kind == "BinaryExpr":
            operator = getattr(node, "attributes", {}).get("operator")
            if operator in {"and", "or", "&&", "||"}:
                decision_count += 1
        for child in getattr(node, "children", []):
            walk(child)

    walk(root)
    return decision_count + 1


def _count_boolean_operators(node) -> int:
    """Count chained boolean operators (and/or)."""
    count = 1
    for child in node.children:
        if child.type == "boolean_operator":
            count += _count_boolean_operators(child)
    return count


def _extract_function_name(node, source_bytes: bytes) -> str | None:
    for child in node.children:
        if child.type == "identifier":
            return source_bytes[child.start_byte : child.end_byte].decode("utf-8")
    return None


# ---------------------------------------------------------------------------
# UAST-driven (language-neutral) implementation
# ---------------------------------------------------------------------------


def _extract_uast_name(node: UASTNode, source_bytes: bytes) -> str | None:
    """Best-effort identifier extraction for a UAST declaration node.

    Returns ``None`` for callables with no bound identifier of their own
    (e.g. a JS/TS arrow function assigned via ``const f = (x) => {...}``,
    where the name lives on the enclosing ``variable_declarator``, not the
    ``FunctionDecl`` node itself) -- these are skipped by callers, matching
    the pre-existing behavior of never picking up anonymous callables
    (Python lambdas were never matched by the native node-type query
    either).
    """
    for child in node.children:
        if child.native.node_kind in _NAME_NATIVE_KINDS:
            return source_bytes[child.span.start_byte : child.span.end_byte].decode(
                "utf-8", errors="replace"
            )
    return None


def _is_async_uast(node: UASTNode, source_bytes: bytes) -> bool:
    """Detect a leading ``async`` modifier on a UAST declaration node.

    tree-sitter models ``async`` as an unnamed leading token, so it is
    filtered out of the UAST tree entirely (only *named* children survive
    mapping) -- but it remains part of the declaration node's own byte span,
    so a text sniff at the start of that span recovers it uniformly across
    Python (``async def``), JS/TS (``async function`` / ``async () =>`` /
    ``async foo()``), and Rust (``async fn``). Go has no ``async`` keyword.
    """
    start = node.span.start_byte
    return bool(_ASYNC_PREFIX_RE.match(source_bytes[start : start + 6]))


def _classify_uast_kind(
    own_kind: str, scope_chain: list[tuple[str, str]], is_async: bool
) -> str:
    if own_kind == "MethodDecl":
        return "method"
    if scope_chain:
        enclosing_kind = scope_chain[-1][0]
        if enclosing_kind == "TypeDecl":
            return "method"
        if enclosing_kind in _FUNCTION_UAST_KINDS:
            return "closure"
    return "async_function" if is_async else "function"


@dataclass(frozen=True)
class _UastFunctionEntry:
    node: UASTNode
    name: str
    qualified_name: str
    kind: str


def _iter_uast_function_entries(
    root: UASTNode, source_bytes: bytes
) -> list[_UastFunctionEntry]:
    """Top-down walk of the UAST collecting every named callable, in
    document order, with its dotted qualified name and method/closure/
    function/async_function classification.

    ``UASTNode`` has no parent pointer, so the enclosing-scope chain is
    threaded down explicitly as the recursion descends (mirroring the
    native fallback's upward ``.parent`` walk, just inverted).
    """
    entries: list[_UastFunctionEntry] = []

    def walk(node: UASTNode, scope_chain: list[tuple[str, str]]) -> None:
        kind = getattr(node, "kind", "")
        next_scope_chain = scope_chain

        if kind in _FUNCTION_UAST_KINDS:
            name = _extract_uast_name(node, source_bytes)
            if name is not None:
                qualified = ".".join([n for _, n in scope_chain] + [name])
                is_async = _is_async_uast(node, source_bytes)
                entries.append(
                    _UastFunctionEntry(
                        node=node,
                        name=name,
                        qualified_name=qualified,
                        kind=_classify_uast_kind(kind, scope_chain, is_async),
                    )
                )
            next_scope_chain = [*scope_chain, (kind, name or "<anon>")]
        elif kind in _SCOPE_UAST_KINDS:  # TypeDecl
            name = _extract_uast_name(node, source_bytes)
            next_scope_chain = [*scope_chain, (kind, name or "<anon>")]

        for child in getattr(node, "children", []):
            walk(child, next_scope_chain)

    walk(root, [])
    return entries


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def calculate_function_complexities(ast: ProgramObject) -> dict[str, int]:
    """
    Calculate cyclomatic complexity for each function in the AST.

    Args:
        ast: The ProgramObject to analyze.

    Returns:
        A dictionary mapping function names to their complexity scores.
    """
    if ast.uast_root is not None:
        source_bytes = ast.source.encode("utf-8")
        return {
            entry.name: _calculate_cyclomatic_complexity_uast(entry.node)
            for entry in _iter_uast_function_entries(ast.uast_root, source_bytes)
        }

    # Legacy fallback for ProgramObjects constructed without a uast_root.
    # Python-only, same as before this fix; unreachable for any ProgramObject
    # produced by ProgramMorphism / parse_source, which always populate
    # uast_root for every supported language.
    complexities: dict[str, int] = {}
    source_bytes = ast.source.encode("utf-8")

    for node in ast.nodes_of_type("function_definition", "async_function_definition"):
        func_name = _extract_function_name(node, source_bytes)
        if func_name is None:
            continue

        func_ast = ProgramObject(
            root=node,
            source=ast.source,
            language=ast.language,
        )
        complexities[func_name] = _calculate_node_complexity(func_ast)

    return complexities


def calculate_max_function_complexity(ast: ProgramObject) -> int:
    """Calculate the maximum cyclomatic complexity found in any function."""
    complexities = calculate_function_complexities(ast)
    return max(complexities.values()) if complexities else 0


@dataclass(frozen=True)
class FunctionComplexity:
    """A single function's complexity with its source location and scope kind.

    Drives ``ast.max_function_complexity`` location reporting: ``complexity`` is
    computed with the exact same decision-node logic as
    ``calculate_function_complexities`` (so the max here equals the gate metric),
    and the span/kind let agents map a failing gate back to a concrete edit
    target. ``includes_nested`` is always ``True`` because the AST count walks
    the whole function subtree, including nested callables.
    """

    name: str
    qualified_name: str
    kind: str
    start_line: int
    end_line: int
    complexity: int
    includes_nested: bool = True


def _scope_chain(node, source_bytes: bytes) -> list[tuple[str, str]]:
    """Enclosing ``(type, name)`` scopes, outermost first (excludes ``node``)."""
    chain: list[tuple[str, str]] = []
    parent = node.parent
    while parent is not None:
        if parent.type in _SCOPE_NODE_TYPES:
            chain.append(
                (parent.type, _extract_function_name(parent, source_bytes) or "<anon>")
            )
        parent = parent.parent
    chain.reverse()
    return chain


def _is_async(node) -> bool:
    # tree-sitter-python models ``async def`` as a ``function_definition`` with
    # a leading ``async`` child token (not a distinct node type).
    return node.type == "async_function_definition" or any(
        child.type == "async" for child in node.children
    )


def _classify_kind(node, scope_chain: list[tuple[str, str]]) -> str:
    if scope_chain:
        enclosing_type = scope_chain[-1][0]
        if enclosing_type == "class_definition":
            return "method"
        if enclosing_type in _FUNCTION_NODE_TYPES:
            return "closure"
    return "async_function" if _is_async(node) else "function"


def calculate_function_complexity_entries(
    ast: ProgramObject,
) -> list[FunctionComplexity]:
    """Per-function complexity with locations, parallel to the gate metric.

    Same decision-node counting as ``calculate_function_complexities`` but
    retains every callable (including same-named ones) plus its span, dotted
    qualified name, and scope kind.
    """
    if ast.uast_root is not None:
        source_bytes = ast.source.encode("utf-8")
        return [
            FunctionComplexity(
                name=entry.name,
                qualified_name=entry.qualified_name,
                kind=entry.kind,
                start_line=entry.node.span.start_line,
                end_line=entry.node.span.end_line,
                complexity=_calculate_cyclomatic_complexity_uast(entry.node),
            )
            for entry in _iter_uast_function_entries(ast.uast_root, source_bytes)
        ]

    # Legacy fallback for ProgramObjects constructed without a uast_root.
    # Python-only, same as before this fix; see calculate_function_complexities.
    source_bytes = ast.source.encode("utf-8")
    entries: list[FunctionComplexity] = []

    for node in ast.nodes_of_type(*_FUNCTION_NODE_TYPES):
        name = _extract_function_name(node, source_bytes)
        if name is None:
            continue
        scope_chain = _scope_chain(node, source_bytes)
        qualified = ".".join([n for _, n in scope_chain] + [name])
        func_ast = ProgramObject(root=node, source=ast.source, language=ast.language)
        entries.append(
            FunctionComplexity(
                name=name,
                qualified_name=qualified,
                kind=_classify_kind(node, scope_chain),
                start_line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                complexity=_calculate_node_complexity(func_ast),
            )
        )

    return entries
