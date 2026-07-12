"""Tests for the shared GitNexus depgraph-generation helper."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from topos.utils.gitnexus import (
    DEFAULT_ANALYZE_TIMEOUT_S,
    GITNEXUS_FINGERPRINT_FILE,
    _resolve_timeout,
    generate_depgraph,
    source_fingerprint,
)

_REAL_RUN = subprocess.run


def _init_repo(root: Path) -> str:
    """Init a git repo with one commit; return its HEAD SHA."""
    subprocess.run(["git", "-C", str(root), "init"], check=True, capture_output=True)
    subprocess.run(
        ["git", "-C", str(root), "config", "user.email", "t@t.t"],
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "-C", str(root), "config", "user.name", "t"],
        check=True,
        capture_output=True,
    )
    (root / "f.py").write_text("x = 1\n", encoding="utf-8")
    subprocess.run(
        ["git", "-C", str(root), "add", "-A"], check=True, capture_output=True
    )
    subprocess.run(
        ["git", "-C", str(root), "commit", "-m", "init"],
        check=True,
        capture_output=True,
    )
    out = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return out.stdout.strip()


def _fake_analyze(cmd, *args, **kwargs):
    """Fake ``gitnexus analyze`` but run real git for everything else."""
    if cmd and cmd[0] == "gitnexus":
        proc = MagicMock()
        proc.returncode = 0
        proc.stdout = "ok"
        proc.stderr = ""
        return proc
    return _REAL_RUN(cmd, *args, **kwargs)


def test_missing_gitnexus_returns_structured_failure(tmp_path: Path) -> None:
    with patch("topos.utils.gitnexus.gitnexus_available", return_value=False):
        result = generate_depgraph(tmp_path)
    assert result.ok is False
    assert result.returncode == 127
    assert "npm install -g gitnexus" in result.message


def test_timeout_is_converted_to_structured_failure(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text("", encoding="utf-8")
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch(
            "subprocess.run",
            side_effect=subprocess.TimeoutExpired(cmd="gitnexus", timeout=300.0),
        ),
    ):
        result = generate_depgraph(tmp_path)
    assert result.ok is False
    assert result.returncode == 124
    assert "timed out" in result.message


def test_oserror_is_converted_to_structured_failure(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text("", encoding="utf-8")
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=OSError("permission denied")),
    ):
        result = generate_depgraph(tmp_path)
    assert result.ok is False
    assert result.returncode == 126
    assert "could not be executed" in result.message


def test_default_timeout_passed_to_subprocess(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text("", encoding="utf-8")
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run") as mock_run,
    ):
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = "ok"
        generate_depgraph(tmp_path)
    # The first subprocess call is the gitnexus analyze (a later call is the
    # best-effort git rev-parse for the fingerprint, which has its own timeout).
    assert mock_run.call_args_list[0].kwargs["timeout"] == DEFAULT_ANALYZE_TIMEOUT_S


def test_env_var_overrides_and_disables_timeout(monkeypatch) -> None:
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "42")
    assert _resolve_timeout(None) == 42.0
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "0")  # non-positive disables
    assert _resolve_timeout(None) is None
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "garbage")  # falls back
    assert _resolve_timeout(None) == DEFAULT_ANALYZE_TIMEOUT_S
    # An explicit argument wins over the env var.
    assert _resolve_timeout(10.0) == 10.0


def test_generate_writes_fingerprint_with_head_sha(tmp_path) -> None:
    head = _init_repo(tmp_path)
    (tmp_path / ".gitnexus").mkdir()  # gitnexus would create this; we mock analyze
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=_fake_analyze),
    ):
        result = generate_depgraph(tmp_path)
    assert result.ok is True
    marker = tmp_path / ".gitnexus" / GITNEXUS_FINGERPRINT_FILE
    assert marker.exists()
    payload = json.loads(marker.read_text(encoding="utf-8"))
    assert payload["head_sha"] == head
    # v2 marker: generation time enables working-tree freshness.
    assert isinstance(payload["generated_at"], float)
    assert payload["generated_at"] > 0
    assert payload["source_hash"] == source_fingerprint(tmp_path).content_hash
    assert payload["source_file_count"] == 1


def test_generate_in_non_git_dir_writes_sha_less_fingerprint(tmp_path) -> None:
    # A non-git directory is a supported target: generation still succeeds and
    # gets a sha-less v2 marker so mtime-based freshness works there too.
    (tmp_path / ".gitnexus").mkdir()
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=_fake_analyze),
    ):
        result = generate_depgraph(tmp_path)
    assert result.ok is True
    marker = tmp_path / ".gitnexus" / GITNEXUS_FINGERPRINT_FILE
    payload = json.loads(marker.read_text(encoding="utf-8"))
    assert payload["head_sha"] is None
    assert payload["generated_at"] > 0
    assert payload["source_hash"] == source_fingerprint(tmp_path).content_hash


def test_generate_skips_unreadable_source_dirs(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    (tmp_path / "main.py").write_text("", encoding="utf-8")
    blocked = tmp_path / "blocked"
    blocked.mkdir()
    (blocked / "hidden.py").write_text("", encoding="utf-8")
    (tmp_path / ".gitnexus").mkdir()
    original_is_dir = Path.is_dir
    original_is_file = Path.is_file

    def fake_is_dir(path: Path) -> bool:
        if path == blocked:
            raise PermissionError("blocked")
        return original_is_dir(path)

    def fake_is_file(path: Path) -> bool:
        if path == blocked:
            raise PermissionError("blocked")
        return original_is_file(path)

    monkeypatch.setattr(Path, "is_dir", fake_is_dir)
    monkeypatch.setattr(Path, "is_file", fake_is_file)
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=_fake_analyze),
    ):
        result = generate_depgraph(tmp_path)

    assert result.ok is True
    marker = tmp_path / ".gitnexus" / GITNEXUS_FINGERPRINT_FILE
    payload = json.loads(marker.read_text(encoding="utf-8"))
    assert payload["source_file_count"] == 1
    assert payload["source_hash"] == source_fingerprint(tmp_path).content_hash
