"""Contract invariant: next_tool/next_actions never contradict blocked_by.

For every COMPOSABLE setup blocker, a contract that also carries refactor
targets must keep the blocker in ``blocked_by`` AND surface its remedy in
``next_actions`` alongside the edit step — the regression guarded here is the
old post-hoc overwrite that erased the setup guidance.
"""

from __future__ import annotations

import pytest
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.mcp.evaluation import INVALID_GITNEXUS_MARKERS, STALE_GITNEXUS_MARKER
from topos.mcp.formatting import build_agent_contract
from topos.mcp.schemas import RefactorTarget


def _target() -> RefactorTarget:
    return RefactorTarget(
        target_id="rt_deadbeef0123",
        kind="function",
        filepath="mod.py",
        symbol="big",
        line_start=1,
        failing_generators=["simple"],
        metric="ast.max_function_complexity",
        current_value=13.0,
        threshold=10.0,
        severity="fix",
        recommended_operations=["extract_helper"],
    )


_BLOCKER_CASES = [
    pytest.param(None, "missing_gitnexus_dir", "topos_generate_depgraph", id="missing"),
    pytest.param(
        [f"{STALE_GITNEXUS_MARKER} — graph predates HEAD"],
        "stale_gitnexus_dir",
        "topos_generate_depgraph",
        id="stale",
    ),
    pytest.param(
        [f"{INVALID_GITNEXUS_MARKERS[0]} — override must be inside the root"],
        "invalid_gitnexus_dir",
        "fix gitnexus_dir",
        id="invalid",
    ),
]


@pytest.mark.parametrize(("warnings", "blocker", "remedy"), _BLOCKER_CASES)
def test_targets_never_erase_setup_blocker_remedy(
    warnings: list[str] | None, blocker: str, remedy: str
) -> None:
    result = ClassificationResult(
        is_parseable=True,
        raw_metrics={"ast.max_function_complexity": 13.0},
    )
    next_tool, next_actions, blocked_by, _, _ = build_agent_contract(
        result,
        coupling_available=False,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=warnings,
        refactor_targets=[_target()],
    )

    assert next_tool == "topos_assess_worktree_change"
    assert blocker in blocked_by
    assert any(a.startswith("edit target rt_") for a in next_actions)
    assert any(remedy in a for a in next_actions), (remedy, next_actions)


def test_no_blocker_yields_edit_step_only() -> None:
    result = ClassificationResult(
        is_parseable=True,
        raw_metrics={"ast.max_function_complexity": 13.0},
    )
    next_tool, next_actions, blocked_by, _, _ = build_agent_contract(
        result,
        coupling_available=True,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        refactor_targets=[_target()],
    )

    assert next_tool == "topos_assess_worktree_change"
    assert blocked_by == []
    assert any(a.startswith("edit target rt_") for a in next_actions)
    assert not any("topos_generate_depgraph" in a for a in next_actions)


def test_requested_but_empty_targets_fall_back_to_ladder() -> None:
    result = ClassificationResult(
        is_parseable=True,
        raw_metrics={"ast.max_function_complexity": 13.0},
    )
    next_tool, next_actions, _, _, _ = build_agent_contract(
        result,
        coupling_available=True,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        refactor_targets=[],
    )

    assert next_tool != "topos_assess_worktree_change"
    # Requested-but-empty must not re-advertise the option either.
    assert not any("refactor_targets=5" in a for a in next_actions)
