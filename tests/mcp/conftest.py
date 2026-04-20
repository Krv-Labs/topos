"""Shared fixtures for MCP tests."""

from __future__ import annotations

from pathlib import Path

import pytest


@pytest.fixture(autouse=True)
def _project_root_env(monkeypatch: pytest.MonkeyPatch) -> None:
    """Every MCP test runs with FILE_ROOT pinned to the repo root."""
    repo_root = Path(__file__).resolve().parents[2]
    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(repo_root))

    # Invalidate the cached root and dep-graph caches between tests.
    from topos.mcp import cache, security

    security.reset_file_root_cache()
    cache.clear_caches()
