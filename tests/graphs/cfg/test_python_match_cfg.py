"""
Python-specific `match`/`case` CFG shape regression tests.

Python's tree-sitter grammar wraps `match` arms similarly to Rust:
`match_statement` -> [subject, block], and that `block`'s children are
`case_clause` nodes.  Prior to this fix, `_match_arms()` didn't recognize
`case_clause` as an arm boundary, so every Python `match` silently
collapsed to a single unconditional edge regardless of case count — the
same class of bug reported for Rust in issue #151, confirmed to affect
Python too during investigation.

These tests pin down the corrected CFG shape so future grammar-mapping or
builder changes don't silently regress it.
"""

from __future__ import annotations

from collections import Counter

from topos.core.morphism import ProgramMorphism
from topos.graphs.cfg.models import EdgeKind
from topos.graphs.cfg.object import ControlFlowGraph


def _cfg(source: str) -> ControlFlowGraph:
    morphism = ProgramMorphism(source=source, language="python")
    cfg = morphism.build_cfg()
    assert cfg is not None
    return cfg


def test_python_match_creates_one_arm_per_case():
    src = (
        "def classify(n):\n"
        "    match n:\n"
        "        case 0:\n"
        '            return "zero"\n'
        "        case 1:\n"
        '            return "one"\n'
        "        case _:\n"
        '            return "other"\n'
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 3
    assert kinds.count(EdgeKind.RETURN) == 3


def test_python_match_with_multistatement_case_keeps_one_arm_per_case():
    """A `case` body with multiple statements must still contribute exactly
    one SWITCH_CASE edge — regression guard against flattening case bodies
    into the enclosing statement list."""
    src = (
        "def classify(n):\n"
        "    match n:\n"
        "        case 0:\n"
        "            do_a()\n"
        "            do_b()\n"
        '            return "zero"\n'
        "        case 1:\n"
        '            return "one"\n'
        "        case _:\n"
        '            return "other"\n'
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 3
    assert kinds.count(EdgeKind.RETURN) == 3


def test_python_match_surfaces_nested_if_inside_case():
    src = (
        "def classify(n, y):\n"
        "    match n:\n"
        "        case 0:\n"
        "            if y > 0:\n"
        '                return "pos"\n'
        "            else:\n"
        '                return "nonpos"\n'
        "        case 1:\n"
        '            return "one"\n'
        "        case _:\n"
        '            return "other"\n'
    )
    cfg = _cfg(src)
    kinds = Counter(e.kind for e in cfg.edges)
    assert kinds[EdgeKind.SWITCH_CASE] == 3
    assert kinds[EdgeKind.TRUE] == 1
    assert kinds[EdgeKind.FALSE] == 1
