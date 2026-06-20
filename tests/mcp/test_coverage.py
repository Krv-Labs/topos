"""Tests for topos_calculate_coverage MCP tool."""

from __future__ import annotations

from topos.mcp.schemas import CalculateCoverageInput, CoverageResult
from topos.mcp.tools.coverage import topos_calculate_coverage


def _coverage(tool_result) -> CoverageResult:
    """Rebuild the CoverageResult model from a tool's ToolResult channel."""
    return CoverageResult.model_validate(tool_result.structured_content)


def test_calculate_coverage_uast_and_topological() -> None:
    inp = CalculateCoverageInput(
        put_files=["tests/fixtures/ect_coverage/tiny_put.py"],
        test_files=["tests/fixtures/ect_coverage/tiny_test.py"],
        k=3,
    )
    result = _coverage(topos_calculate_coverage(inp))

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


def test_calculate_coverage_markdown_content_and_structured() -> None:
    inp = CalculateCoverageInput(
        put_files=["tests/fixtures/ect_coverage/tiny_put.py"],
        test_files=["tests/fixtures/ect_coverage/tiny_test.py"],
        k=3,
    )
    tr = topos_calculate_coverage(inp)
    text = tr.content[0].text
    # Content block is compact markdown, NOT serialized JSON.
    assert not text.lstrip().startswith("{")
    assert text.lstrip().startswith("# Structural Test Coverage")
    # Structured channel still carries the model for programmatic clients.
    assert tr.structured_content is not None
    assert "mean_declaration_coverage" in tr.structured_content
