"""Tests for security.py — file-root resolution, fail-closed default, path checks."""

from __future__ import annotations

from pathlib import Path

import pytest

from topos.mcp import security


def test_resolve_root_uses_env_var(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    assert security.resolve_file_root() == tmp_path.resolve()


def test_resolve_root_auto_detects_via_git_marker(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.delenv("TOPOS_MCP_FILE_ROOT", raising=False)
    (tmp_path / ".git").mkdir()
    sub = tmp_path / "a" / "b"
    sub.mkdir(parents=True)
    monkeypatch.chdir(sub)
    security.reset_file_root_cache()
    assert security.resolve_file_root() == tmp_path.resolve()


def test_resolve_root_fails_closed_when_nothing_found(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """The critical security fix — no env var AND no project marker → error."""
    monkeypatch.delenv("TOPOS_MCP_FILE_ROOT", raising=False)
    monkeypatch.chdir(tmp_path)
    security.reset_file_root_cache()
    with pytest.raises(security.FileRootNotConfiguredError):
        security.resolve_file_root()


def test_read_safe_utf8_file_rejects_outside(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # Pin root to an inner directory, try to read a file from outside it.
    inner = tmp_path / "inner"
    inner.mkdir()
    outside = tmp_path / "outside.py"
    outside.write_text("x = 1")
    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(inner))
    security.reset_file_root_cache()
    _, err = security.read_safe_utf8_file(outside)
    assert err is not None
    assert "Access denied" in err["error"]


def test_read_safe_utf8_file_allows_inside(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    inside = tmp_path / "inside.py"
    inside.write_text("x = 1")
    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    src, err = security.read_safe_utf8_file(inside)
    assert err is None
    assert src == "x = 1"
