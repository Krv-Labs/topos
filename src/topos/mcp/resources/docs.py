# ruff: noqa: E501
"""
Documentation resources served over MCP.

Content lives in ``./content/*.md`` and is returned verbatim — no runtime
string-building. Agents fetch these on first encounter to understand the
lattice, metrics, priority profiles, and the canonical refactor loop.

Line-length is waived because the docstrings (which become resource
descriptions shown to clients) read better as single sentences.
"""

from __future__ import annotations

from pathlib import Path

from ..server import mcp

_CONTENT_DIR = Path(__file__).parent / "content"


def _read(name: str) -> str:
    return (_CONTENT_DIR / name).read_text(encoding="utf-8")


@mcp.resource(
    "topos://docs/lattice",
    name="topos_lattice_reference",
    mime_type="text/markdown",
)
def lattice_reference() -> str:
    """The diamond lattice: BROKEN / COMPOSABLE / SELF_CONTAINED / SOUND, and why COMPOSABLE and SELF_CONTAINED are incomparable."""
    return _read("lattice.md")


@mcp.resource(
    "topos://docs/metrics",
    name="topos_metrics_reference",
    mime_type="text/markdown",
)
def metrics_reference() -> str:
    """Every metric key, good ranges, and how they roll up into dimension scores."""
    return _read("metrics.md")


@mcp.resource(
    "topos://docs/priority",
    name="topos_priority_reference",
    mime_type="text/markdown",
)
def priority_reference() -> str:
    """Priority profiles (balanced / composable / self_contained) and when to use each."""
    return _read("priority.md")


@mcp.resource(
    "topos://docs/workflows",
    name="topos_workflow_guide",
    mime_type="text/markdown",
)
def workflow_guide() -> str:
    """The canonical agent refactor loop: review → plan → refactor → re-measure. Read first."""
    return _read("workflows.md")
