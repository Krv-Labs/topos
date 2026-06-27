"""Tests for topos_get_doc — tool-based fallback for clients without resource access."""

from __future__ import annotations

import pytest
from topos.mcp.tools.docs import topos_get_doc


def test_get_doc_returns_workflows() -> None:
    body = topos_get_doc(topic="workflows")
    assert "review → plan → refactor → re-measure" in body
    assert "SUSPICIOUS_NO_STRUCTURAL_CHANGE" in body
    assert '"params"' in body
    assert "acknowledged_risks" in body
    assert "agent_contract" in body
    assert "if unavailable or not run" in body
    assert "Always verify behavior with tests" not in body


def test_get_doc_returns_agent_contract() -> None:
    body = topos_get_doc(topic="agent-contract")
    assert "Done Gates" in body
    assert "agent_contract" in body
    assert "verification_gates" in body


def test_get_doc_points_refactors_to_compact_contract_first() -> None:
    doc = topos_get_doc.__doc__ or ""
    assert "agent-contract" in doc
    assert "Read first for refactors" in doc
    assert "Read this first on every new refactor session" not in doc


def test_get_doc_returns_lattice() -> None:
    body = topos_get_doc(topic="lattice")
    assert "Evaluation Lattice" in body
    assert "IDEAL" in body


def test_get_doc_returns_metrics() -> None:
    body = topos_get_doc(topic="metrics")
    assert "cfg.cyclomatic" in body
    assert "mdg.coupling" in body
    assert "cpg.dangerous_calls" in body


def test_get_doc_returns_priority() -> None:
    body = topos_get_doc(topic="priority")
    assert "secure" in body
    assert "composable" in body


def test_get_doc_returns_preferences() -> None:
    body = topos_get_doc(topic="preferences")
    assert "preference_walk" in body
    assert '"params"' in body


def test_get_doc_matches_resource_content() -> None:
    """Same source file; tool and resource must return byte-identical content."""
    import asyncio

    import topos.mcp.prompts  # noqa: F401
    import topos.mcp.resources  # noqa: F401
    import topos.mcp.tools  # noqa: F401
    from topos.mcp.server import mcp

    for topic in (
        "agent-contract",
        "lattice",
        "metrics",
        "preferences",
        "priority",
        "workflows",
    ):
        via_tool = topos_get_doc(topic=topic)
        result = asyncio.run(mcp.read_resource(f"topos://docs/{topic}"))
        resource_body = "".join(
            getattr(c, "content", getattr(c, "text", str(c))) for c in result.contents
        )
        assert via_tool == resource_body, f"drift between tool and resource for {topic}"


def test_get_doc_rejects_unknown_topic() -> None:
    # Literal-typed param; passing an unknown topic should error when reading
    # the missing file off disk.
    with pytest.raises(FileNotFoundError):
        topos_get_doc(topic="not_a_real_topic")  # type: ignore[arg-type]
