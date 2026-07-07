"""
Go-specific CFG shape regression tests.

Go's tree-sitter grammar has two shapes not exercised by any other
currently-supported language:

1. Function/if/for bodies wrap their statements in an intermediate
   ``statement_list`` node (``block`` -> ``statement_list`` -> statements),
   one level deeper than Python's ``suite`` or C++'s ``compound_statement``.
2. ``switch``/``select`` can omit a discriminant entirely (``switch { case
   ...}``, ``select { case ... }``), so the first non-predicate child of the
   statement is itself a case arm rather than a subject expression.

These tests pin down the resulting CFG shape so future grammar-mapping or
builder changes don't silently regress them.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.graphs.cfg.models import EdgeKind
from topos.graphs.cfg.object import ControlFlowGraph


def _cfg(source: str) -> ControlFlowGraph:
    morphism = ProgramMorphism(source=source, language="go")
    cfg = morphism.build_cfg()
    assert cfg is not None
    return cfg


def test_go_if_else_if_chain_creates_nested_decision():
    src = (
        "package main\n\n"
        "func classify(n int) string {\n"
        "\tif n < 0 {\n"
        "\t\treturn \"neg\"\n"
        "\t} else if n == 0 {\n"
        "\t\treturn \"zero\"\n"
        "\t} else {\n"
        "\t\treturn \"other\"\n"
        "\t}\n"
        "}\n"
    )
    cfg = _cfg(src)
    # Two independent if-decisions (outer + nested else-if) => 3, plus 1 for
    # the synthetic module-level callable that every real .go file triggers
    # (its `package` clause is a non-function top-level child, same as any
    # other language's top-level statements outside a function).
    assert cfg.metrics()["cfg.cyclomatic"] == 4.0
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.TRUE) == 2
    assert kinds.count(EdgeKind.FALSE) == 2
    assert kinds.count(EdgeKind.RETURN) == 3


def test_go_tagless_switch_creates_one_arm_per_case():
    src = (
        "package main\n\n"
        "func classify(n int) string {\n"
        "\tswitch {\n"
        "\tcase n < 0:\n"
        "\t\treturn \"neg\"\n"
        "\tcase n == 0:\n"
        "\t\treturn \"zero\"\n"
        "\tdefault:\n"
        "\t\treturn \"pos\"\n"
        "\t}\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 3
    assert kinds.count(EdgeKind.RETURN) == 3


def test_go_type_switch_creates_one_arm_per_case():
    src = (
        "package main\n\n"
        "func typesw(x interface{}) string {\n"
        "\tswitch x.(type) {\n"
        "\tcase int:\n"
        "\t\treturn \"int\"\n"
        "\tdefault:\n"
        "\t\treturn \"other\"\n"
        "\t}\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 2
    assert kinds.count(EdgeKind.RETURN) == 2


def test_go_select_statement_creates_one_arm_per_case():
    src = (
        "package main\n\n"
        "func sel(ch chan int, done chan bool) int {\n"
        "\tselect {\n"
        "\tcase v := <-ch:\n"
        "\t\treturn v\n"
        "\tcase <-done:\n"
        "\t\treturn 0\n"
        "\t}\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 2
    assert kinds.count(EdgeKind.RETURN) == 2


def test_go_if_with_init_clause_keeps_then_branch():
    """`if x := f(); cond {}` has no wrapper grouping the init statement
    with the condition, so the then-block is not the second child — it
    must be located by kind, not position."""
    src = (
        "package main\n\n"
        "func f(err error) int {\n"
        "\tif err := g(); err != nil {\n"
        "\t\treturn 1\n"
        "\t}\n"
        "\treturn 0\n"
        "}\n"
    )
    cfg = _cfg(src)
    then_block = next(b for b in cfg.blocks.values() if b.label == "if_then")
    assert any(s.kind == "ReturnStmt" for s in then_block.statements)


def test_go_for_loop_break_continue_resolve_to_loop_targets():
    src = (
        "package main\n\n"
        "func loopy(items []int) int {\n"
        "\ttotal := 0\n"
        "\tfor i := 0; i < len(items); i++ {\n"
        "\t\tif items[i] < 0 {\n"
        "\t\t\tcontinue\n"
        "\t\t}\n"
        "\t\tif items[i] == 0 {\n"
        "\t\t\tbreak\n"
        "\t\t}\n"
        "\t\ttotal += items[i]\n"
        "\t}\n"
        "\treturn total\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = {e.kind for e in cfg.edges}
    assert EdgeKind.LOOP_BACK in kinds
    assert EdgeKind.BREAK in kinds
    assert EdgeKind.CONTINUE in kinds
