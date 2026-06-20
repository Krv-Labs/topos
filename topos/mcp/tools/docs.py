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

from ..server import mcp

_CONTENT_DIR = Path(__file__).parent.parent / "resources" / "content"

DocTopic = Literal[
    "agent-contract", "lattice", "metrics", "preferences", "priority", "workflows"
]


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
        agent-contract — compact outcome-first loop contract and done gates.
        lattice    — the 8-element 3-cube H(G_qual); top = IDEAL, bottom = SLOP.
        metrics    — every metric key, thresholds, interpretation.
        preferences — strict generator rankings and preference walks.
        priority   — priority profiles (simple/composable/secure).
        workflows  — the canonical review→plan→refactor→re-measure loop.
                     **Read this first on every new refactor session.**
    """
    path = _CONTENT_DIR / f"{topic}.md"
    return path.read_text(encoding="utf-8")
