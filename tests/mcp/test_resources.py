"""Tests for the documentation resources."""

from __future__ import annotations

import asyncio

import topos.mcp.prompts  # noqa: F401
import topos.mcp.resources  # noqa: F401
import topos.mcp.tools  # noqa: F401
from topos.mcp.server import mcp


def test_docs_resources_registered() -> None:
    resources = asyncio.run(mcp.list_resources())
    uris = {str(r.uri) for r in resources}
    assert "topos://docs/agent-contract" in uris
    assert "topos://docs/lattice" in uris
    assert "topos://docs/metrics" in uris
    assert "topos://docs/preferences" in uris
    assert "topos://docs/priority" in uris
    assert "topos://docs/workflows" in uris


def test_lattice_resource_returns_markdown() -> None:
    blocks = asyncio.run(mcp.read_resource("topos://docs/lattice"))
    contents = "".join(b.content if hasattr(b, "content") else str(b) for b in blocks)
    assert "Evaluation Lattice" in contents
    assert "IDEAL" in contents
    assert "COMPOSABLE" in contents
    assert "SIMPLE" in contents
    assert "SECURE" in contents


def test_workflows_resource_has_refactor_loop() -> None:
    blocks = asyncio.run(mcp.read_resource("topos://docs/workflows"))
    contents = "".join(b.content if hasattr(b, "content") else str(b) for b in blocks)
    assert "review → plan → refactor → re-measure" in contents
    assert "SUSPICIOUS_NO_STRUCTURAL_CHANGE" in contents


def test_agent_contract_resource_has_done_gates() -> None:
    blocks = asyncio.run(mcp.read_resource("topos://docs/agent-contract"))
    contents = "".join(b.content if hasattr(b, "content") else str(b) for b in blocks)
    assert "Done Gates" in contents
    assert "agent_contract" in contents
