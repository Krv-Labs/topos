"""Tests for the depgraph status/generation tools and stale contract wiring."""

from __future__ import annotations

import pytest
from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.base import Priority
from topos.mcp.evaluation import DepgraphStatus
from topos.mcp.formatting import build_agent_contract
from topos.mcp.schemas import (
    DepgraphState,
    DepgraphStatusInput,
    DepgraphStatusResult,
    GenerateDepgraphInput,
    GenerateDepgraphResult,
)
from topos.mcp.tools import depgraph as depgraph_tool
from topos.mcp.tools.depgraph import topos_depgraph_status, topos_generate_depgraph
from topos.utils.gitnexus import DepgraphGenerationResult


def _status(tool_result) -> DepgraphStatusResult:
    return DepgraphStatusResult.model_validate(tool_result.structured_content)


def _generate(tool_result) -> GenerateDepgraphResult:
    return GenerateDepgraphResult.model_validate(tool_result.structured_content)


def _use_root(tmp_path, monkeypatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()


def test_status_missing_points_to_generate(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)  # tmp_path has no .gitnexus
    r = _status(topos_depgraph_status(DepgraphStatusInput()))
    assert r.state == DepgraphState.MISSING
    assert r.coupling_available is False
    assert r.agent_contract.next_tool == "topos_generate_depgraph"
    assert "missing_gitnexus_dir" in r.agent_contract.blocked_by


@pytest.mark.parametrize(
    ("state", "blocked", "coupling", "next_tool"),
    [
        (DepgraphState.PRESENT, [], True, "topos_evaluate_file"),
        (DepgraphState.STALE, ["stale_gitnexus_dir"], False, "topos_generate_depgraph"),
        (
            DepgraphState.LOAD_ERROR,
            ["gitnexus_load_error"],
            False,
            "topos_generate_depgraph",
        ),
        (
            DepgraphState.SCHEMA_MISMATCH,
            ["gitnexus_schema_mismatch"],
            False,
            "topos_generate_depgraph",
        ),
        # An invalid override must NOT route to generation (a bad path won't be
        # fixed by regenerating); next_tool is None so the agent fixes the path.
        (DepgraphState.INVALID_DIR, ["invalid_gitnexus_dir"], False, None),
    ],
)
def test_status_maps_each_state(
    tmp_path, monkeypatch, state, blocked, coupling, next_tool
) -> None:
    _use_root(tmp_path, monkeypatch)

    def fake_status(override, project_root, target_file):
        return DepgraphStatus(
            state=state.value,
            gitnexus_dir=str(tmp_path / ".gitnexus"),
            gitnexus_mtime=1.0,
            git_head_mtime=2.0,
            detail="x",
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    r = _status(topos_depgraph_status(DepgraphStatusInput()))
    assert r.state == state
    assert r.coupling_available is coupling
    assert r.agent_contract.blocked_by == blocked
    assert r.agent_contract.next_tool == next_tool
    # The state-specific code is mirrored into risk_flags, not just blocked_by,
    # so clients branching on risk_flags alone can tell the states apart.
    for code in blocked:
        assert code in r.agent_contract.risk_flags
    if state != DepgraphState.PRESENT:
        assert "composable_unavailable" in r.agent_contract.risk_flags


def test_generate_success(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"

    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda d: DepgraphGenerationResult(True, 0, gitnexus, "done"),
    )
    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))
    assert r.ok is True
    assert r.gitnexus_dir == str(gitnexus)
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_generate_failure_when_gitnexus_missing(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda d: DepgraphGenerationResult(False, 127, None, "GitNexus not found."),
    )
    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))
    assert r.ok is False
    assert r.error == "GitNexus not found."
    assert "gitnexus_generate_failed" in r.agent_contract.blocked_by


def test_build_agent_contract_flags_stale_graph() -> None:
    result = ClassificationResult(
        is_parseable=True,
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.SLOP,
            "secure": EvaluationValue.SECURE,
        },
        scores={"simple": 1.0, "composable": 0.0, "secure": 1.0},
        lattice_element=EvaluationValue.SIMPLE_SECURE,
        priority=Priority.SIMPLE,
    )
    next_tool, _actions, blocked_by, _gates, risk_flags = build_agent_contract(
        result,
        coupling_available=True,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=["gitnexus index may be stale — regenerate before trusting."],
    )
    assert "stale_gitnexus_dir" in blocked_by
    assert "stale_gitnexus_dir" in risk_flags
    assert next_tool == "topos_generate_depgraph"


def test_build_agent_contract_flags_invalid_override() -> None:
    result = ClassificationResult(
        is_parseable=True,
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.SLOP,
            "secure": EvaluationValue.SECURE,
        },
        scores={"simple": 1.0, "composable": 0.0, "secure": 1.0},
        lattice_element=EvaluationValue.SIMPLE_SECURE,
        priority=Priority.SIMPLE,
    )
    next_tool, _actions, blocked_by, _gates, risk_flags = build_agent_contract(
        result,
        coupling_available=False,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=["gitnexus_dir rejected — override must be inside the root."],
    )
    # A bad override is invalid_gitnexus_dir, not missing_gitnexus_dir, and must
    # NOT be routed to generation.
    assert "invalid_gitnexus_dir" in blocked_by
    assert "missing_gitnexus_dir" not in blocked_by
    assert "invalid_gitnexus_dir" in risk_flags
    assert next_tool != "topos_generate_depgraph"


def test_status_invalid_gitnexus_dir_end_to_end(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    bogus = tmp_path / "does-not-exist"  # inside root but nonexistent
    r = _status(topos_depgraph_status(DepgraphStatusInput(gitnexus_dir=str(bogus))))
    assert r.state == DepgraphState.INVALID_DIR
    assert r.coupling_available is False
    assert "invalid_gitnexus_dir" in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool != "topos_generate_depgraph"
