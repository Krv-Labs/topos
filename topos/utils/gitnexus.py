"""Shared GitNexus dependency-graph generation.

A single place for the ``gitnexus analyze`` invocation so the CLI
(``topos depgraph generate``) and the MCP tool (``topos_generate_depgraph``)
stay in lockstep.
"""

from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

GITNEXUS_CMD = "gitnexus"
_INSTALL_HINT = "GitNexus not found. Install it with: npm install -g gitnexus"


def gitnexus_available() -> bool:
    """Whether the ``gitnexus`` CLI is on PATH."""
    return shutil.which(GITNEXUS_CMD) is not None


@dataclass(frozen=True)
class DepgraphGenerationResult:
    """Outcome of a ``gitnexus analyze`` run."""

    ok: bool
    returncode: int
    gitnexus_path: Path | None
    message: str


def generate_depgraph(
    target_dir: Path, *, capture: bool = True
) -> DepgraphGenerationResult:
    """Run ``gitnexus analyze --skip-agents-md`` in ``target_dir``.

    ``capture=False`` streams gitnexus output to the inherited stdio (used by
    the CLI); ``capture=True`` collects it into ``message`` (used by MCP).
    """
    if not gitnexus_available():
        return DepgraphGenerationResult(False, 127, None, _INSTALL_HINT)

    proc = subprocess.run(
        [GITNEXUS_CMD, "analyze", "--skip-agents-md"],
        cwd=target_dir,
        capture_output=capture,
        text=True,
    )
    if proc.returncode != 0:
        detail = ""
        if capture:
            detail = (proc.stderr or proc.stdout or "").strip()
        return DepgraphGenerationResult(
            False,
            proc.returncode,
            None,
            detail or "gitnexus analyze failed.",
        )

    gitnexus_path = target_dir / ".gitnexus"
    detail = (proc.stdout or "").strip() if capture else ""
    return DepgraphGenerationResult(
        True,
        0,
        gitnexus_path,
        detail or f"Dependency graph written to {gitnexus_path}",
    )
