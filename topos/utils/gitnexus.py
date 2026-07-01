"""Shared GitNexus dependency-graph generation.

A single place for the ``gitnexus analyze`` invocation so the CLI
(``topos depgraph generate``) and the MCP tool (``topos_generate_depgraph``)
stay in lockstep.
"""

from __future__ import annotations

import contextlib
import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

GITNEXUS_CMD = "gitnexus"
_INSTALL_HINT = "GitNexus not found. Install it with: npm install -g gitnexus"

# Topos-owned marker written inside ``.gitnexus`` recording the HEAD commit the
# graph was built from. Freshness is anchored to this commit rather than file
# mtimes (``gitnexus analyze`` does not reliably bump its DB mtime on re-index),
# so a regenerate always clears the "stale" signal. Read back by
# ``topos.mcp.evaluation`` — keep the filename here as the single source of truth.
GITNEXUS_FINGERPRINT_FILE = ".topos-fingerprint.json"

# ``gitnexus analyze`` can legitimately run for minutes on a large repo, so the
# default ceiling is deliberately generous. Operators can override it via the
# ``TOPOS_DEPGRAPH_TIMEOUT`` env var (seconds); set it to 0 to disable entirely.
DEFAULT_ANALYZE_TIMEOUT_S = 300.0
_TIMEOUT_RC = 124  # conventional "timed out" exit code
_EXEC_ERROR_RC = 126  # command found but could not be executed


def _resolve_timeout(timeout: float | None) -> float | None:
    """Resolve the effective ``subprocess`` timeout in seconds.

    ``None`` (the default) falls back to ``TOPOS_DEPGRAPH_TIMEOUT`` or
    ``DEFAULT_ANALYZE_TIMEOUT_S``. A non-positive value disables the timeout.
    """
    if timeout is not None:
        return timeout if timeout > 0 else None
    raw = os.environ.get("TOPOS_DEPGRAPH_TIMEOUT")
    if raw is None:
        return DEFAULT_ANALYZE_TIMEOUT_S
    try:
        parsed = float(raw)
    except ValueError:
        return DEFAULT_ANALYZE_TIMEOUT_S
    return parsed if parsed > 0 else None


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
    target_dir: Path, *, capture: bool = True, timeout: float | None = None
) -> DepgraphGenerationResult:
    """Run ``gitnexus analyze --skip-agents-md`` in ``target_dir``.

    ``capture=False`` streams gitnexus output to the inherited stdio (used by
    the CLI); ``capture=True`` collects it into ``message`` (used by MCP).

    ``timeout`` bounds the subprocess in seconds; ``None`` uses
    ``TOPOS_DEPGRAPH_TIMEOUT`` or ``DEFAULT_ANALYZE_TIMEOUT_S``. A hung or
    unrunnable ``gitnexus`` is converted into a structured failure result rather
    than blocking the caller or raising, so callers always get a deterministic
    ``(ok, returncode, message)``.
    """
    if not gitnexus_available():
        return DepgraphGenerationResult(False, 127, None, _INSTALL_HINT)

    effective_timeout = _resolve_timeout(timeout)
    try:
        proc = subprocess.run(
            [GITNEXUS_CMD, "analyze", "--skip-agents-md"],
            cwd=target_dir,
            capture_output=capture,
            text=True,
            timeout=effective_timeout,
        )
    except subprocess.TimeoutExpired:
        limit = f"{effective_timeout:.0f}s" if effective_timeout else "the limit"
        return DepgraphGenerationResult(
            False,
            _TIMEOUT_RC,
            None,
            f"gitnexus analyze timed out after {limit}; raise "
            "TOPOS_DEPGRAPH_TIMEOUT or run it manually.",
        )
    except OSError as exc:
        return DepgraphGenerationResult(
            False,
            _EXEC_ERROR_RC,
            None,
            f"gitnexus analyze could not be executed: {exc}",
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
    _write_fingerprint(target_dir, gitnexus_path)
    detail = (proc.stdout or "").strip() if capture else ""
    return DepgraphGenerationResult(
        True,
        0,
        gitnexus_path,
        detail or f"Dependency graph written to {gitnexus_path}",
    )


def _head_sha(target_dir: Path) -> str | None:
    """Current HEAD commit SHA, or ``None`` when there is no resolvable commit.

    A missing ``.git``, an unborn HEAD (no commits yet), or a detached HEAD with
    no ref are all normal for the directories GitNexus can analyze — treat them
    as "no fingerprint" rather than an error.
    """
    try:
        proc = subprocess.run(
            ["git", "-C", str(target_dir), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if proc.returncode != 0:
        return None
    sha = proc.stdout.strip()
    return sha or None


def _write_fingerprint(target_dir: Path, gitnexus_path: Path) -> None:
    """Record the HEAD commit the graph was built from (best-effort).

    Never raises: a non-git directory simply gets no marker (freshness falls back
    to mtime), and a write failure must not turn a successful generation into a
    failure.
    """
    sha = _head_sha(target_dir)
    if not isinstance(sha, str) or not sha:
        return
    # Best-effort: a read-only FS (OSError) or unexpected payload (Type/ValueError
    # from json) must never turn a successful generation into a failure.
    with contextlib.suppress(OSError, TypeError, ValueError):
        (gitnexus_path / GITNEXUS_FINGERPRINT_FILE).write_text(
            json.dumps({"head_sha": sha}), encoding="utf-8"
        )
