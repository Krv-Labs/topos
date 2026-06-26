"""Tests for the edit-in-place assessment tools.

Covers ``topos_begin_refactor`` + ``topos_assess_snapshot`` (content-addressed
snapshot store) and ``topos_assess_worktree_change`` (stateless git baseline).
"""

from __future__ import annotations

import hashlib
import os
import stat
import subprocess
from pathlib import Path

import pytest
from topos.mcp.schemas import (
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    AssessSnapshotInput,
    AssessWorktreeChangeInput,
    BeginRefactorInput,
    SnapshotResult,
)
from topos.mcp.tools.assess import snapshot as assess_snapshot_mod
from topos.mcp.tools.assess.core import topos_assess_improvement
from topos.mcp.tools.assess.snapshot import (
    topos_assess_snapshot,
    topos_begin_refactor,
)
from topos.mcp.tools.assess.worktree import topos_assess_worktree_change

_BASE = "def f(x):\n    return x + 1\n"
_EDIT = "def f(x):\n    return x + 2\n"


def _root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Pin FILE_ROOT and the snapshot store to a fresh temp dir."""
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    monkeypatch.setenv("TOPOS_SNAPSHOT_DIR", str(tmp_path / ".snaps"))
    security.reset_file_root_cache()
    return tmp_path


def _assessment(tool_result) -> AssessmentResult:
    return AssessmentResult.model_validate(tool_result.structured_content)


def _snapshot(tool_result) -> SnapshotResult:
    return SnapshotResult.model_validate(tool_result.structured_content)


def _sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


# ---------------------------------------------------------------------------
# Snapshot flow
# ---------------------------------------------------------------------------


def test_begin_refactor_reports_baseline_hash(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    (tmp_path / "m.py").write_text(_BASE, encoding="utf-8")

    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="m.py")))

    assert snap.error is None
    assert snap.baseline_hash == _sha(_BASE)  # pure content hash
    assert len(snap.snapshot_id) == 64  # opaque handle, keyed by (file, content)
    assert snap.agent_contract is not None
    assert snap.agent_contract.next_tool == "topos_assess_snapshot"


def test_identical_content_distinct_files_dont_collide(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Two files with identical baseline source get independent handles."""
    _root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text(_BASE, encoding="utf-8")
    (tmp_path / "b.py").write_text(_BASE, encoding="utf-8")

    snap_a = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="a.py")))
    snap_b = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="b.py")))
    assert snap_a.snapshot_id != snap_b.snapshot_id

    # a.py's handle still resolves to a.py after b.py was captured.
    (tmp_path / "a.py").write_text(_EDIT, encoding="utf-8")
    r = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id=snap_a.snapshot_id, filepath="a.py")
        )
    )
    assert r.error is None
    assert r.baseline_hash == _sha(_BASE)


def test_snapshot_roundtrip_after_in_place_edit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    target = tmp_path / "m.py"
    target.write_text(_BASE, encoding="utf-8")

    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="m.py")))
    target.write_text(_EDIT, encoding="utf-8")  # edit in place — baseline gone

    r = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id=snap.snapshot_id, filepath="m.py")
        )
    )

    assert r.error is None
    assert r.status in set(AssessmentStatus)
    assert r.baseline_hash == _sha(_BASE)
    assert r.current_hash == _sha(_EDIT)
    assert r.structural_distance is not None


@pytest.mark.skipif(os.name != "posix", reason="POSIX permission bits")
def test_snapshot_store_is_private(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Baseline source must not be world-readable on shared temp dirs."""
    _root(tmp_path, monkeypatch)
    (tmp_path / "m.py").write_text(_BASE, encoding="utf-8")
    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="m.py")))

    store = tmp_path / ".snaps"
    project_dir = next(store.iterdir())  # <hash>/ namespace dir
    assert stat.S_IMODE(project_dir.stat().st_mode) == 0o700
    blob = project_dir / f"{snap.snapshot_id}.blob"
    assert stat.S_IMODE(blob.stat().st_mode) == 0o600


def test_assess_snapshot_missing_id(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    (tmp_path / "m.py").write_text(_BASE, encoding="utf-8")

    r = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id="0" * 64, filepath="m.py")
        )
    )

    assert r.error is not None
    assert r.agent_contract is not None
    assert "snapshot_not_found" in r.agent_contract.blocked_by


def test_assess_snapshot_wrong_file_is_stale(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text(_BASE, encoding="utf-8")
    (tmp_path / "b.py").write_text(_BASE, encoding="utf-8")

    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="a.py")))
    # Same content, different file → the snapshot is bound to a.py.
    r = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id=snap.snapshot_id, filepath="b.py")
        )
    )

    assert r.agent_contract is not None
    assert "snapshot_stale" in r.agent_contract.blocked_by


def test_assess_snapshot_expired_is_stale(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    target = tmp_path / "m.py"
    target.write_text(_BASE, encoding="utf-8")

    monkeypatch.setattr(assess_snapshot_mod, "snapshot_now", lambda: 1000.0)
    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="m.py")))
    target.write_text(_EDIT, encoding="utf-8")

    # Jump past the 24h TTL on the assessment read.
    monkeypatch.setattr(assess_snapshot_mod, "snapshot_now", lambda: 1000.0 + 25 * 3600)
    r = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id=snap.snapshot_id, filepath="m.py")
        )
    )

    assert r.agent_contract is not None
    assert "snapshot_stale" in r.agent_contract.blocked_by


def test_snapshot_status_parity_with_assess_improvement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Snapshot assessment must yield the same status as the side-by-side tool."""
    _root(tmp_path, monkeypatch)
    target = tmp_path / "m.py"
    target.write_text(_BASE, encoding="utf-8")

    snap = _snapshot(topos_begin_refactor(BeginRefactorInput(filepath="m.py")))
    target.write_text(_EDIT, encoding="utf-8")
    snap_status = _assessment(
        topos_assess_snapshot(
            AssessSnapshotInput(snapshot_id=snap.snapshot_id, filepath="m.py")
        )
    ).status

    improvement_status = _assessment(
        topos_assess_improvement(
            AssessImprovementInput(current_code=_BASE, proposed_code=_EDIT)
        )
    ).status

    assert snap_status == improvement_status


# ---------------------------------------------------------------------------
# Worktree flow (git baseline)
# ---------------------------------------------------------------------------


def _git(repo: Path, *args: str) -> None:
    # -c flags supply an identity so commits work without any global gitconfig.
    subprocess.run(
        ["git", "-C", str(repo), "-c", "user.email=t@t", "-c", "user.name=t", *args],
        check=True,
        capture_output=True,
    )


def _git_available() -> bool:
    try:
        subprocess.run(["git", "--version"], capture_output=True, check=True, timeout=2)
        return True
    except (FileNotFoundError, OSError, subprocess.CalledProcessError):
        return False


pytestmark_git = pytest.mark.skipif(not _git_available(), reason="git not installed")


@pytestmark_git
def test_worktree_change_vs_head(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    _git(tmp_path, "init")
    target = tmp_path / "m.py"
    target.write_text(_BASE, encoding="utf-8")
    _git(tmp_path, "add", "m.py")
    _git(tmp_path, "commit", "-m", "base")
    target.write_text(_EDIT, encoding="utf-8")  # edit working tree

    r = _assessment(
        topos_assess_worktree_change(AssessWorktreeChangeInput(filepath="m.py"))
    )

    assert r.error is None
    assert r.status in set(AssessmentStatus)
    assert r.baseline_hash == _sha(_BASE)  # baseline read from HEAD
    assert r.current_hash == _sha(_EDIT)


@pytestmark_git
def test_worktree_bad_ref(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _root(tmp_path, monkeypatch)
    _git(tmp_path, "init")
    (tmp_path / "m.py").write_text(_BASE, encoding="utf-8")
    _git(tmp_path, "add", "m.py")
    _git(tmp_path, "commit", "-m", "base")

    r = _assessment(
        topos_assess_worktree_change(
            AssessWorktreeChangeInput(filepath="m.py", baseline_ref="nope")
        )
    )

    assert r.agent_contract is not None
    assert "baseline_ref_not_found" in r.agent_contract.blocked_by


def test_worktree_not_a_git_repo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _root(tmp_path, monkeypatch)
    (tmp_path / "m.py").write_text(_BASE, encoding="utf-8")

    r = _assessment(
        topos_assess_worktree_change(AssessWorktreeChangeInput(filepath="m.py"))
    )

    assert r.agent_contract is not None
    assert "not_a_git_repo" in r.agent_contract.blocked_by
