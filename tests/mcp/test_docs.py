"""Tests for topos_get_doc — tool-based fallback for clients without resource access."""

from __future__ import annotations

import pytest

from topos.mcp.tools.docs import topos_get_doc


def test_get_doc_returns_workflows() -> None:
    body = topos_get_doc(topic="workflows")
    assert "review → plan → refactor → re-measure" in body
    assert "SUSPICIOUS_NO_STRUCTURAL_CHANGE" in body


def test_get_doc_returns_lattice() -> None:
    body = topos_get_doc(topic="lattice")
    assert "Diamond Lattice" in body
    assert "SOUND" in body


def test_get_doc_returns_metrics() -> None:
    body = topos_get_doc(topic="metrics")
    assert "ast.complexity" in body
    assert "depgraph.coupling" in body


def test_get_doc_returns_priority() -> None:
    body = topos_get_doc(topic="priority")
    assert "balanced" in body
    assert "composable" in body


def test_get_doc_matches_resource_content() -> None:
    """Same source file; tool and resource must return byte-identical content."""
    import asyncio

    from topos.mcp import mcp

    for topic in ("lattice", "metrics", "priority", "workflows"):
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
