"""
Documentation tool — read the Topos docs as an MCP tool.

Exists because MCP resources (``topos://docs/*``) are not uniformly reachable
from agents across clients in 2026. Claude Code bridges resources to the
model implicitly; Gemini CLI (and others) do not. This tool exposes the same
markdown content via a tool call so critical context (especially the
workflow guide) is reachable everywhere.

Content is read verbatim from ``../resources/content/*.md`` — single source
of truth shared with the resource handlers.
"""

from __future__ import annotations

from pathlib import Path
from typing import Literal

from topos.mcp.server import mcp

_CONTENT_DIR = Path(__file__).parent.parent / "resources" / "content"

DocTopic = Literal["lattice", "metrics", "priority", "workflows"]


@mcp.tool(
    name="topos_get_doc",
    tags={"docs"},
    annotations={
        "title": "Topos Documentation",
        "readOnlyHint": True,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
def topos_get_doc(topic: DocTopic) -> str:
    """Return a Topos documentation page as Markdown.

    Use when your MCP client does not expose resource fetching to the agent
    (e.g. Gemini CLI). Clients that do surface resources should prefer the
    equivalent resource URI for efficiency: ``topos://docs/{topic}``.

    Topics:
        lattice    — the diamond lattice (BROKEN/COMPOSABLE/SELF_CONTAINED/SOUND).
        metrics    — every metric key, thresholds, interpretation.
        priority   — priority profiles (balanced/composable/self_contained).
        workflows  — the canonical review→plan→refactor→re-measure loop.
                     **Read this first on every new refactor session.**
    """
    path = _CONTENT_DIR / f"{topic}.md"
    return path.read_text(encoding="utf-8")
