"""
Rust-specific CFG shape regression tests.

Rust's tree-sitter grammar wraps `match` arms one level deeper than the
generic case: `match_expression` -> [scrutinee, match_block], and
`match_block`'s children are `match_arm` nodes.  Prior to this fix,
`_match_arms()` didn't recognize `match_block`/`match_arm`, so every Rust
`match` silently collapsed to a single unconditional edge regardless of
arm count (issue #151: an 8-arm match read `cfg.cyclomatic == 1.0`,
identical to straight-line code).

These tests pin down the corrected CFG shape so future grammar-mapping or
builder changes don't silently regress it.
"""

from __future__ import annotations

from collections import Counter

from topos.core.morphism import ProgramMorphism
from topos.graphs.cfg.models import EdgeKind
from topos.graphs.cfg.object import ControlFlowGraph


def _cfg(source: str) -> ControlFlowGraph:
    morphism = ProgramMorphism(source=source, language="rust")
    cfg = morphism.build_cfg()
    assert cfg is not None
    return cfg


def test_rust_eight_arm_match_grows_cyclomatic_complexity():
    """Direct reproduction of issue #151: an 8-arm match must not read as
    cyclomatic complexity 1 (identical to no branching)."""
    src = (
        "pub fn probe(x: u8) -> &'static str {\n"
        "    match x {\n"
        '        0 => "a", 1 => "b", 2 => "c", 3 => "d",\n'
        '        4 => "e", 5 => "f", 6 => "g", _ => "h",\n'
        "    }\n"
        "}\n"
    )
    cfg = _cfg(src)
    assert cfg.metrics()["cfg.cyclomatic"] == 8.0
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 8


def test_rust_match_with_multistatement_arm_keeps_one_arm_per_case():
    """A match arm with a multi-statement block body must still contribute
    exactly one SWITCH_CASE edge — not one per inner statement.  This is a
    regression guard against flattening arm bodies into the enclosing
    statement list (the trap that would occur from naively reusing Go's
    case-arm allowlist for Rust's match_arm nodes)."""
    src = (
        "pub fn probe(x: u8) -> &'static str {\n"
        "    match x {\n"
        '        0 => { do_a(); do_b(); "a" },\n'
        '        1 => "b",\n'
        '        _ => "c",\n'
        "    }\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = [e.kind for e in cfg.edges]
    assert kinds.count(EdgeKind.SWITCH_CASE) == 3
    assert cfg.metrics()["cfg.cyclomatic"] == 3.0


def test_rust_match_surfaces_nested_if_inside_arm():
    """A decision nested inside one arm's body must still be surfaced as
    its own TRUE/FALSE branch, on top of the arm's own SWITCH_CASE edge."""
    src = (
        "pub fn probe(x: u8, y: u8) -> &'static str {\n"
        "    match x {\n"
        '        0 => { if y > 0 { "pos" } else { "nonpos" } },\n'
        '        1 => "b",\n'
        '        _ => "c",\n'
        "    }\n"
        "}\n"
    )
    cfg = _cfg(src)
    kinds = Counter(e.kind for e in cfg.edges)
    assert kinds[EdgeKind.SWITCH_CASE] == 3
    assert kinds[EdgeKind.TRUE] == 1
    assert kinds[EdgeKind.FALSE] == 1
    assert cfg.metrics()["cfg.cyclomatic"] == 4.0
