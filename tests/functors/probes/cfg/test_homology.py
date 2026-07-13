"""Tests for the CFG cycle-basis probe (issue #83)."""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.functors.probes.cfg.homology import calculate_cycle_basis


def _cfg(source: str, language: str = "python"):
    morphism = ProgramMorphism(source=source, language=language)
    cfg = morphism.build_cfg()
    assert cfg is not None
    return cfg


def test_no_branches_has_zero_cycles():
    cfg = _cfg("def f(): return 1")
    result = calculate_cycle_basis(cfg)
    assert result.betti_1 == 0
    assert result.cycles == []


def test_single_loop_yields_one_cycle_covering_its_lines():
    source = (
        "def f(items):\n"
        "    total = 0\n"
        "    for x in items:\n"
        "        total += x\n"
        "    return total\n"
    )
    cfg = _cfg(source)
    result = calculate_cycle_basis(cfg)
    assert result.betti_1 == 1
    assert len(result.cycles) == 1
    cycle = result.cycles[0]
    assert cycle.start_line is not None
    assert cycle.end_line is not None
    # The loop body ("total += x") lives on line 4.
    assert cycle.start_line <= 4 <= cycle.end_line


def test_betti_1_matches_cyclomatic_complexity_minus_one():
    source = (
        "def f(items):\n"
        "    total = 0\n"
        "    for x in items:\n"
        "        if x > 0:\n"
        "            total += x\n"
        "    return total\n"
    )
    cfg = _cfg(source)
    result = calculate_cycle_basis(cfg)
    cyclomatic = cfg.metrics()["cfg.cyclomatic"]
    assert result.betti_1 == int(cyclomatic) - 1


def test_cycles_are_not_folded_into_metrics():
    cfg = _cfg("def f(): return 1")
    m = cfg.metrics()
    assert not any("homology" in k or "cycle" in k for k in m)
