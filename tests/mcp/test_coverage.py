"""Tests for topos_calculate_coverage MCP tool."""

from __future__ import annotations

from topos.mcp.schemas import CalculateCoverageInput
from topos.mcp.tools.coverage import topos_calculate_coverage


def test_calculate_coverage_uast_and_topological() -> None:
    inp = CalculateCoverageInput(
        put_files=["tests/fixtures/ect_coverage/tiny_put.py"],
        test_files=["tests/fixtures/ect_coverage/tiny_test.py"],
        k=3,
    )
    result = topos_calculate_coverage(inp)

    assert result.error is None
    assert result.put_declaration_count >= 1
    assert result.mean_declaration_coverage >= 0.0
    assert result.topological_coverage is not None

    topo = result.topological_coverage
    if topo.unavailable:
        assert topo.reason
        assert "ect-coverage" in topo.reason.lower()
    else:
        assert topo.coverage_score is not None
        assert topo.scoped_node_count is not None
