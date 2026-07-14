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
    current_git_branch,
    generate_depgraph,
    resolve_lbug_store,
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


def _checkout_branch(root: Path, name: str) -> None:
    subprocess.run(
        ["git", "-C", str(root), "checkout", "-b", name],
        check=True,
        capture_output=True,
    )


def _write_meta(store_dir: Path, *, branch: str, last_commit: str = "abc123") -> None:
    store_dir.mkdir(parents=True, exist_ok=True)
    (store_dir / "meta.json").write_text(
        json.dumps({"branch": branch, "lastCommit": last_commit}), encoding="utf-8"
    )


def _write_flat_store(gitnexus_dir: Path, *, branch: str | None) -> Path:
    gitnexus_dir.mkdir(parents=True, exist_ok=True)
    lbug = gitnexus_dir / "lbug"
    lbug.write_bytes(b"\x00")
    if branch is not None:
        _write_meta(gitnexus_dir, branch=branch)
    return lbug


def _write_branch_store(
    gitnexus_dir: Path, *, branch: str, suffix: str = "deadbeef"
) -> Path:
    store_dir = gitnexus_dir / "branches" / f"{branch}-{suffix}"
    store_dir.mkdir(parents=True, exist_ok=True)
    lbug = store_dir / "lbug"
    lbug.write_bytes(b"\x00")
    _write_meta(store_dir, branch=branch)
    return lbug


# ---------------------------------------------------------------------------
# current_git_branch
# ---------------------------------------------------------------------------


def test_current_git_branch_reads_symbolic_ref(tmp_path: Path) -> None:
    _init_repo(tmp_path)
    _checkout_branch(tmp_path, "feature-x")
    assert current_git_branch(tmp_path) == "feature-x"


def test_current_git_branch_none_for_detached_head(tmp_path: Path) -> None:
    _init_repo(tmp_path)
    head_sha = subprocess.run(
        ["git", "-C", str(tmp_path), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    subprocess.run(
        ["git", "-C", str(tmp_path), "checkout", head_sha],
        check=True,
        capture_output=True,
    )
    assert current_git_branch(tmp_path) is None


def test_current_git_branch_none_without_git_dir(tmp_path: Path) -> None:
    assert current_git_branch(tmp_path) is None


# ---------------------------------------------------------------------------
# resolve_lbug_store
# ---------------------------------------------------------------------------


def test_resolve_lbug_store_flat_matches_current_branch(tmp_path: Path) -> None:
    gitnexus_dir = tmp_path / ".gitnexus"
    flat = _write_flat_store(gitnexus_dir, branch="main")
    resolved = resolve_lbug_store(gitnexus_dir, "main")
    assert resolved.path == flat
    assert resolved.matched_branch == "main"


def test_resolve_lbug_store_prefers_branch_dir_over_wrong_flat(tmp_path: Path) -> None:
    gitnexus_dir = tmp_path / ".gitnexus"
    _write_flat_store(gitnexus_dir, branch="main")
    branch_lbug = _write_branch_store(gitnexus_dir, branch="feature-x")
    resolved = resolve_lbug_store(gitnexus_dir, "feature-x")
    assert resolved.path == branch_lbug
    assert resolved.matched_branch == "feature-x"


def test_resolve_lbug_store_no_match_reports_available_branches(tmp_path: Path) -> None:
    gitnexus_dir = tmp_path / ".gitnexus"
    _write_flat_store(gitnexus_dir, branch="main")
    _write_branch_store(gitnexus_dir, branch="feature-x")
    resolved = resolve_lbug_store(gitnexus_dir, "feature-y")
    assert resolved.path is None
    assert resolved.available_branches == ("feature-x", "main")


def test_resolve_lbug_store_none_branch_uses_flat_unconditionally(
    tmp_path: Path,
) -> None:
    gitnexus_dir = tmp_path / ".gitnexus"
    flat = _write_flat_store(gitnexus_dir, branch="main")
    _write_branch_store(gitnexus_dir, branch="feature-x")
    resolved = resolve_lbug_store(gitnexus_dir, None)
    assert resolved.path == flat


def test_resolve_lbug_store_legacy_flat_with_no_meta_used_unconditionally(
    tmp_path: Path,
) -> None:
    gitnexus_dir = tmp_path / ".gitnexus"
    flat = _write_flat_store(gitnexus_dir, branch=None)  # no meta.json at all
    resolved = resolve_lbug_store(gitnexus_dir, "feature-x")
    assert resolved.path == flat


def test_resolve_lbug_store_missing_gitnexus_dir(tmp_path: Path) -> None:
    resolved = resolve_lbug_store(tmp_path / ".gitnexus", "main")
    assert resolved.path is None
    assert resolved.available_branches == ()


# ---------------------------------------------------------------------------
# Fingerprint colocation (the false-negative regression from the bug report)
# ---------------------------------------------------------------------------


def test_generate_writes_fingerprint_beside_resolved_branch_store(
    tmp_path: Path,
) -> None:
    _init_repo(tmp_path)
    _checkout_branch(tmp_path, "feature-x")
    gitnexus_dir = tmp_path / ".gitnexus"
    # Flat slot already belongs to another branch (main) with its own stale
    # fingerprint; a branch-scoped store for feature-x already exists (as if
    # GitNexus had just written it) but has no fingerprint yet.
    _write_flat_store(gitnexus_dir, branch="main")
    (gitnexus_dir / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps({"head_sha": "stale-main-sha"}), encoding="utf-8"
    )
    branch_lbug = _write_branch_store(gitnexus_dir, branch="feature-x")

    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=_fake_analyze),
    ):
        result = generate_depgraph(tmp_path)

    assert result.ok is True
    branch_fingerprint = branch_lbug.parent / GITNEXUS_FINGERPRINT_FILE
    assert branch_fingerprint.exists()
    payload = json.loads(branch_fingerprint.read_text(encoding="utf-8"))
    assert payload["head_sha"] is not None
    # The flat slot's fingerprint must be left untouched (still main's stale sha).
    flat_payload = json.loads(
        (gitnexus_dir / GITNEXUS_FINGERPRINT_FILE).read_text(encoding="utf-8")
    )
    assert flat_payload["head_sha"] == "stale-main-sha"
