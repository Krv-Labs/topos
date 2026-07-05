"""Smoke tests for MCP documentation bundled in frozen binaries."""

from __future__ import annotations

import asyncio
import os
from datetime import timedelta
from pathlib import Path

import pytest
from mcp.client.session import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client

REPO_ROOT = Path(__file__).resolve().parents[2]
CONTENT_DIR = REPO_ROOT / "topos" / "mcp" / "resources" / "content"
DOC_TOPICS = (
    "agent-contract",
    "lattice",
    "metrics",
    "preferences",
    "priority",
    "workflows",
)


def test_mcp_doc_content_files_exist_in_repo() -> None:
    for topic in DOC_TOPICS:
        path = CONTENT_DIR / f"{topic}.md"
        assert path.is_file(), f"missing doc content file: {path}"


async def _fetch_workflows_doc(binary: str) -> str:
    """Call topos_get_doc(workflows) over MCP stdio using the official client."""
    params = StdioServerParameters(command=binary, args=["mcp"])
    async with stdio_client(params) as (read, write):  # noqa: SIM117
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "topos_get_doc",
                {"topic": "workflows"},
                read_timeout_seconds=timedelta(seconds=60),
            )
        assert not result.isError, result
        chunks: list[str] = []
        for block in result.content:
            text = getattr(block, "text", None)
            if text:
                chunks.append(text)
        return "\n".join(chunks)


@pytest.mark.skipif(
    not os.environ.get("TOPOS_BINARY"),
    reason="Set TOPOS_BINARY to smoke-test MCP docs in a PyInstaller binary",
)
def test_frozen_binary_topos_get_doc_workflows() -> None:
    """Invoke topos_get_doc against a built binary via MCP stdio."""
    binary = os.environ["TOPOS_BINARY"]
    timeout_s = float(os.environ.get("TOPOS_MCP_SMOKE_TIMEOUT_S", "180"))
    text = asyncio.run(
        asyncio.wait_for(_fetch_workflows_doc(binary), timeout=timeout_s)
    )
    assert "workflows" in text.lower() or "review" in text.lower(), text[:200]
