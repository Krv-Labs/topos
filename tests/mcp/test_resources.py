"""Tests for the documentation resources."""

from __future__ import annotations

import asyncio

from topos.mcp import mcp


def test_all_four_docs_resources_registered() -> None:
    resources = asyncio.run(mcp.list_resources())
    uris = {str(r.uri) for r in resources}
    assert "topos://docs/lattice" in uris
    assert "topos://docs/metrics" in uris
    assert "topos://docs/priority" in uris
    assert "topos://docs/workflows" in uris


def test_lattice_resource_returns_markdown() -> None:
    blocks = asyncio.run(mcp.read_resource("topos://docs/lattice"))
    contents = "".join(b.content if hasattr(b, "content") else str(b) for b in blocks)
    assert "Diamond Lattice" in contents
    assert "SOUND" in contents
    assert "COMPOSABLE" in contents
    assert "SELF_CONTAINED" in contents


def test_workflows_resource_has_refactor_loop() -> None:
    blocks = asyncio.run(mcp.read_resource("topos://docs/workflows"))
    contents = "".join(b.content if hasattr(b, "content") else str(b) for b in blocks)
    assert "review → plan → refactor → re-measure" in contents
    assert "SUSPICIOUS_NO_STRUCTURAL_CHANGE" in contents
