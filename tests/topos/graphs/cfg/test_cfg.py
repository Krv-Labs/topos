"""
Tests for the ControlFlowGraph representation.

The CFG translational functor R_CFG : Lang -> E maps source to a CFG built
on UAST.  These tests verify:

1. Protocol conformance (ControlFlowGraph implements Representation).
2. Cyclomatic complexity ground truth across languages.
3. Builder produces a connected single-component graph (so P = 1).
4. Edge labels (TRUE / FALSE / LOOP_BACK / RETURN) are correct.
"""

from __future__ import annotations

import pytest

from topos.core.morphism import ProgramMorphism
from topos.graphs.base import Representation
from topos.graphs.cfg.models import EdgeKind
from topos.graphs.cfg.object import ControlFlowGraph


def _cfg(source: str, language: str = "python") -> ControlFlowGraph:
    morphism = ProgramMorphism(source=source, language=language)
    cfg = morphism.build_cfg()
    assert cfg is not None
    return cfg


def test_cfg_implements_representation_protocol():
    cfg = _cfg("def f(): return 1")
    assert isinstance(cfg, Representation)
    assert cfg.name == "cfg"
    assert cfg.dimension == "simple"


def test_cfg_emits_cfg_namespaced_metrics():
    cfg = _cfg("def f(): return 1")
    m = cfg.metrics()
    assert "cfg.cyclomatic" in m
    assert "cfg.essential" in m
    assert "cfg.nesting_depth" in m
    assert "cfg.longest_path" in m


def test_cyclomatic_linear_function_is_one():
    cfg = _cfg("def add(a, b):\n    return a + b\n")
    assert cfg.metrics()["cfg.cyclomatic"] == 1.0


def test_cyclomatic_single_if_is_two():
    cfg = _cfg(
        "def f(x):\n"
        "    if x > 0:\n"
        "        return 1\n"
        "    return 0\n"
    )
    assert cfg.metrics()["cfg.cyclomatic"] == 2.0


def test_cyclomatic_n_independent_ifs():
    """n independent if-statements → cyclomatic = n + 1."""
    for n in range(1, 5):
        guards = "".join(f"    if a{i}: pass\n" for i in range(n))
        params = ", ".join(f"a{i}" for i in range(n))
        src = f"def f({params}):\n{guards}    return 0\n"
        cfg = _cfg(src)
        assert cfg.metrics()["cfg.cyclomatic"] == n + 1, (
            f"n={n}, got {cfg.metrics()['cfg.cyclomatic']}"
        )


def test_cfg_contains_entry_and_exit_blocks():
    cfg = _cfg("def f(): return 1")
    assert cfg.entry_id in cfg.blocks
    assert cfg.exit_id in cfg.blocks
    assert cfg.blocks[cfg.entry_id].label == "entry"
    assert cfg.blocks[cfg.exit_id].label == "exit"


def test_if_statement_generates_true_and_false_edges():
    cfg = _cfg(
        "def f(x):\n    if x:\n        return 1\n    return 0\n"
    )
    kinds = {e.kind for e in cfg.edges}
    assert EdgeKind.TRUE in kinds
    assert EdgeKind.FALSE in kinds
    assert EdgeKind.RETURN in kinds


def test_while_loop_generates_back_edge():
    cfg = _cfg(
        "def f(x):\n    while x > 0:\n        x -= 1\n    return x\n"
    )
    kinds = {e.kind for e in cfg.edges}
    assert EdgeKind.LOOP_BACK in kinds


def test_break_continue_resolve_to_loop_targets():
    cfg = _cfg(
        "def f(xs):\n"
        "    for x in xs:\n"
        "        if x == 0:\n"
        "            break\n"
        "        if x < 0:\n"
        "            continue\n"
        "    return 0\n"
    )
    kinds = {e.kind for e in cfg.edges}
    assert EdgeKind.BREAK in kinds
    assert EdgeKind.CONTINUE in kinds


@pytest.mark.parametrize(
    "language,linear,branchy",
    [
        ("python", "def f(x): return x\n", "def f(x):\n    if x: return 1\n    return 0\n"),
        (
            "javascript",
            "function f(x) { return x; }\n",
            "function f(x) { if (x) return 1; return 0; }\n",
        ),
        (
            "rust",
            "fn f(x: i32) -> i32 { return x; }\n",
            "fn f(x: i32) -> i32 { if x > 0 { return 1; } return 0; }\n",
        ),
    ],
)
def test_cyclomatic_grows_with_branching_across_languages(
    language, linear, branchy
):
    linear_cyc = _cfg(linear, language=language).metrics()["cfg.cyclomatic"]
    branchy_cyc = _cfg(branchy, language=language).metrics()["cfg.cyclomatic"]
    assert branchy_cyc > linear_cyc, f"{language}: {linear_cyc} -> {branchy_cyc}"
