"""Tests for the depgraph status/generation tools and stale contract wiring."""

from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path

import pytest
from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.base import Priority
from topos.mcp.cache import clear_caches, dep_graph_for
from topos.mcp.evaluation import (
    STALE_GITNEXUS_MARKER,
    DepgraphStatus,
    _git_head_sha,
    _graph_freshness,
)
from topos.mcp.formatting import build_agent_contract
from topos.mcp.schemas import (
    DepgraphState,
    DepgraphStatusInput,
    DepgraphStatusResult,
    GenerateDepgraphInput,
    GenerateDepgraphResult,
)
from topos.mcp.tools import depgraph as depgraph_tool
from topos.mcp.tools.depgraph import topos_depgraph_status, topos_generate_depgraph
from topos.utils.gitnexus import (
    GITNEXUS_FINGERPRINT_FILE,
    DepgraphGenerationResult,
    source_fingerprint,
)


def _status(tool_result) -> DepgraphStatusResult:
    return DepgraphStatusResult.model_validate(tool_result.structured_content)


def _generate(tool_result) -> GenerateDepgraphResult:
    return GenerateDepgraphResult.model_validate(tool_result.structured_content)


def _use_root(tmp_path, monkeypatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()


def test_status_missing_points_to_generate(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)  # tmp_path has no .gitnexus
    r = _status(topos_depgraph_status(DepgraphStatusInput()))
    assert r.state == DepgraphState.MISSING
    assert r.coupling_available is False
    assert r.agent_contract.next_tool == "topos_generate_depgraph"
    assert "missing_gitnexus_dir" in r.agent_contract.blocked_by


@pytest.mark.parametrize(
    ("state", "blocked", "coupling", "next_tool"),
    [
        (DepgraphState.PRESENT, [], True, "topos_evaluate_file"),
        (DepgraphState.STALE, ["stale_gitnexus_dir"], False, "topos_generate_depgraph"),
        (
            DepgraphState.LOAD_ERROR,
            ["gitnexus_load_error"],
            False,
            "topos_generate_depgraph",
        ),
        # A schema mismatch means the store was written by a NEWER GitNexus
        # than the embedded reader — regenerating cannot fix it, so status must
        # not route to generation (the generate default refuses this state).
        (
            DepgraphState.SCHEMA_MISMATCH,
            ["gitnexus_schema_mismatch"],
            False,
            None,
        ),
        # An invalid override must NOT route to generation (a bad path won't be
        # fixed by regenerating); next_tool is None so the agent fixes the path.
        (DepgraphState.INVALID_DIR, ["invalid_gitnexus_dir"], False, None),
        # A branch with no indexed store (others may be indexed) routes to
        # generation, same shape as MISSING/STALE.
        (
            DepgraphState.BRANCH_NOT_INDEXED,
            ["branch_not_indexed_gitnexus_dir"],
            False,
            "topos_generate_depgraph",
        ),
    ],
)
def test_status_maps_each_state(
    tmp_path, monkeypatch, state, blocked, coupling, next_tool
) -> None:
    _use_root(tmp_path, monkeypatch)

    def fake_status(override, project_root, target_file):
        return DepgraphStatus(
            state=state.value,
            gitnexus_dir=str(tmp_path / ".gitnexus"),
            gitnexus_mtime=1.0,
            git_head_mtime=2.0,
            detail="x",
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    r = _status(topos_depgraph_status(DepgraphStatusInput()))
    assert r.state == state
    assert r.coupling_available is coupling
    assert r.agent_contract.blocked_by == blocked
    assert r.agent_contract.next_tool == next_tool
    # The state-specific code is mirrored into risk_flags, not just blocked_by,
    # so clients branching on risk_flags alone can tell the states apart.
    for code in blocked:
        assert code in r.agent_contract.risk_flags
    if state != DepgraphState.PRESENT:
        assert "composable_unavailable" in r.agent_contract.risk_flags


def test_generate_success(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"

    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda d: DepgraphGenerationResult(True, 0, gitnexus, "done"),
    )
    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))
    assert r.ok is True
    assert r.gitnexus_dir == str(gitnexus)
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_generate_skips_when_graph_present(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"

    def fake_status(_override, _project_root, _target_file):
        return DepgraphStatus(
            state=DepgraphState.PRESENT.value,
            gitnexus_dir=str(gitnexus),
            gitnexus_mtime=1.0,
            git_head_mtime=1.0,
            detail=None,
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda _d: pytest.fail("generate_depgraph should not be called"),
    )

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))

    assert r.ok is True
    assert r.generated is False
    assert r.state_before == DepgraphState.PRESENT
    assert r.gitnexus_dir == str(gitnexus)
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_generate_force_runs_when_graph_present(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"
    calls = []

    def fake_generate(d):
        calls.append(d)
        return DepgraphGenerationResult(True, 0, gitnexus, "done")

    monkeypatch.setattr(
        depgraph_tool,
        "depgraph_status",
        lambda *_args: pytest.fail("force=True should skip the status precheck"),
    )
    monkeypatch.setattr(depgraph_tool, "generate_depgraph", fake_generate)

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput(force=True)))

    assert calls == [tmp_path]
    assert r.ok is True
    assert r.generated is True
    assert r.state_before is None
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_generate_runs_once_when_graph_stale(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"
    calls = []

    def fake_generate(d):
        calls.append(d)
        return DepgraphGenerationResult(True, 0, gitnexus, "done")

    def fake_status(_override, _project_root, _target_file):
        return DepgraphStatus(
            state=DepgraphState.STALE.value,
            gitnexus_dir=str(gitnexus),
            gitnexus_mtime=1.0,
            git_head_mtime=2.0,
            detail="stale",
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    monkeypatch.setattr(depgraph_tool, "generate_depgraph", fake_generate)

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))

    assert calls == [tmp_path]
    assert r.ok is True
    assert r.generated is True
    assert r.state_before == DepgraphState.STALE
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_generate_blocks_schema_mismatch_by_default(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"

    def fake_status(_override, _project_root, _target_file):
        return DepgraphStatus(
            state=DepgraphState.SCHEMA_MISMATCH.value,
            gitnexus_dir=str(gitnexus),
            gitnexus_mtime=1.0,
            git_head_mtime=2.0,
            detail="schema mismatch",
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda _d: pytest.fail("schema mismatch should block by default"),
    )

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))

    assert r.ok is False
    assert r.generated is False
    assert r.state_before == DepgraphState.SCHEMA_MISMATCH
    assert "gitnexus_schema_mismatch" in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool is None


def test_generate_failure_when_gitnexus_missing(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    monkeypatch.setattr(
        depgraph_tool,
        "generate_depgraph",
        lambda d: DepgraphGenerationResult(False, 127, None, "GitNexus not found."),
    )
    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))
    assert r.ok is False
    assert r.error == "GitNexus not found."
    assert r.generated is False
    assert "gitnexus_generate_failed" in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool is None


def test_build_agent_contract_flags_stale_graph() -> None:
    result = ClassificationResult(
        is_parseable=True,
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.SLOP,
            "secure": EvaluationValue.SECURE,
        },
        scores={"simple": 1.0, "composable": 0.0, "secure": 1.0},
        lattice_element=EvaluationValue.SIMPLE_SECURE,
        priority=Priority.SIMPLE,
    )
    next_tool, _actions, blocked_by, _gates, risk_flags = build_agent_contract(
        result,
        coupling_available=True,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=["gitnexus index may be stale — regenerate before trusting."],
    )
    assert "stale_gitnexus_dir" in blocked_by
    assert "stale_gitnexus_dir" in risk_flags
    assert next_tool == "topos_generate_depgraph"


def test_build_agent_contract_routes_stale_ideal_to_generate() -> None:
    result = ClassificationResult(
        is_parseable=True,
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.COMPOSABLE,
            "secure": EvaluationValue.SECURE,
        },
        scores={"simple": 1.0, "composable": 1.0, "secure": 1.0},
        lattice_element=EvaluationValue.IDEAL,
        priority=Priority.SIMPLE,
    )
    next_tool, actions, blocked_by, _gates, risk_flags = build_agent_contract(
        result,
        coupling_available=True,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=["gitnexus index may be stale — regenerate before trusting."],
    )

    assert "stale_gitnexus_dir" in blocked_by
    assert "stale_gitnexus_dir" in risk_flags
    assert next_tool == "topos_generate_depgraph"
    assert actions == ["run topos_generate_depgraph to refresh COMPOSABLE"]


def test_build_agent_contract_flags_invalid_override() -> None:
    result = ClassificationResult(
        is_parseable=True,
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.SLOP,
            "secure": EvaluationValue.SECURE,
        },
        scores={"simple": 1.0, "composable": 0.0, "secure": 1.0},
        lattice_element=EvaluationValue.SIMPLE_SECURE,
        priority=Priority.SIMPLE,
    )
    next_tool, _actions, blocked_by, _gates, risk_flags = build_agent_contract(
        result,
        coupling_available=False,
        security_findings=[],
        acknowledged_risks=[],
        grade_capped=False,
        warnings=["gitnexus_dir rejected — override must be inside the root."],
    )
    # A bad override is invalid_gitnexus_dir, not missing_gitnexus_dir, and must
    # NOT be routed to generation.
    assert "invalid_gitnexus_dir" in blocked_by
    assert "missing_gitnexus_dir" not in blocked_by
    assert "invalid_gitnexus_dir" in risk_flags
    assert next_tool != "topos_generate_depgraph"


def test_status_invalid_gitnexus_dir_end_to_end(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    bogus = tmp_path / "does-not-exist"  # inside root but nonexistent
    r = _status(topos_depgraph_status(DepgraphStatusInput(gitnexus_dir=str(bogus))))
    assert r.state == DepgraphState.INVALID_DIR
    assert r.coupling_available is False
    assert "invalid_gitnexus_dir" in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool != "topos_generate_depgraph"


# --- freshness (commit-SHA anchored, mtime fallback) --------------------------


def _commit_repo(root) -> str:
    def g(*a):
        subprocess.run(["git", "-C", str(root), *a], check=True, capture_output=True)

    g("init")
    g("config", "user.email", "t@t.t")
    g("config", "user.name", "t")
    (root / "f.py").write_text("x = 1\n", encoding="utf-8")
    g("add", "-A")
    g("commit", "-m", "init")
    out = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return out.stdout.strip()


def _graph_dir(root, *, fingerprint: str | None) -> os.PathLike:
    gitnexus = root / ".gitnexus"
    gitnexus.mkdir()
    lbug = gitnexus / "lbug"
    lbug.write_text("", encoding="utf-8")
    os.utime(lbug, (1.0, 1.0))  # deliberately ancient, like an un-bumped DB
    if fingerprint is not None:
        (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
            json.dumps({"head_sha": fingerprint}), encoding="utf-8"
        )
    return gitnexus


def test_git_head_sha_reads_without_shelling(tmp_path) -> None:
    head = _commit_repo(tmp_path)
    assert _git_head_sha(tmp_path) == head


def test_freshness_sha_match_is_fresh_despite_old_mtime(tmp_path) -> None:
    # The exact false-positive we hit: graph DB mtime is old, but its fingerprint
    # matches HEAD, so it must read as fresh (not stale).
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=head)
    is_stale, detail = _graph_freshness(tmp_path, gitnexus)
    assert is_stale is False
    assert detail is None


def test_freshness_sha_mismatch_is_stale(tmp_path) -> None:
    _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint="0" * 40)  # built from another commit
    is_stale, detail = _graph_freshness(tmp_path, gitnexus)
    assert is_stale is True
    assert STALE_GITNEXUS_MARKER in detail


def test_freshness_no_marker_falls_back_to_mtime(tmp_path) -> None:
    _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)  # legacy graph, ancient lbug
    is_stale, detail = _graph_freshness(tmp_path, gitnexus)
    assert is_stale is True
    assert STALE_GITNEXUS_MARKER in detail


def test_freshness_non_git_is_fresh(tmp_path) -> None:
    # No .git and no marker: nothing to be stale against → fresh, no crash.
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    assert _graph_freshness(tmp_path, gitnexus) == (False, None)


# --- fingerprint v2: working-tree freshness via generated_at ------------------


def _write_v2_fingerprint(
    gitnexus, *, head_sha: str | None, generated_at: float
) -> None:
    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps({"head_sha": head_sha, "generated_at": generated_at}),
        encoding="utf-8",
    )


def _write_snapshot_fingerprint(
    root: Path,
    gitnexus: Path,
    *,
    head_sha: str | None,
    generated_at: float = 100.0,
    finished_at: float = 100.0,
) -> None:
    snapshot = source_fingerprint(root)
    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps(
            {
                "head_sha": head_sha,
                "generated_at": generated_at,
                "finished_at": finished_at,
                "source_hash": snapshot.content_hash,
                "source_file_count": snapshot.file_count,
            }
        ),
        encoding="utf-8",
    )


def test_freshness_v2_source_edited_after_generation_is_stale(tmp_path) -> None:
    # The in-place-edit loop: HEAD unchanged, but a source file was modified
    # after the graph was generated — COMPOSABLE reflects the pre-edit tree.
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    source_mtime = (tmp_path / "f.py").stat().st_mtime
    _write_v2_fingerprint(gitnexus, head_sha=head, generated_at=source_mtime - 10)

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert STALE_GITNEXUS_MARKER in detail
    assert "f.py" in detail
    assert "modified after the dependency graph was generated" in detail


def test_freshness_v2_untouched_tree_is_fresh(tmp_path) -> None:
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    source_mtime = (tmp_path / "f.py").stat().st_mtime
    _write_v2_fingerprint(gitnexus, head_sha=head, generated_at=source_mtime + 10)

    assert _graph_freshness(tmp_path, gitnexus) == (False, None)


def test_freshness_v2_sha_mismatch_wins_over_mtime(tmp_path) -> None:
    _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    _write_v2_fingerprint(gitnexus, head_sha="0" * 40, generated_at=2**53)

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert "built from commit" in detail


def test_freshness_v2_sha_less_marker_works_in_non_git_dir(tmp_path) -> None:
    # Non-git analysis dirs get a sha-less v2 marker; the mtime pass still
    # detects post-generation edits there.
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    (tmp_path / "mod.py").write_text("x = 1\n", encoding="utf-8")
    source_mtime = (tmp_path / "mod.py").stat().st_mtime
    _write_v2_fingerprint(gitnexus, head_sha=None, generated_at=source_mtime - 10)

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert "mod.py" in detail


def test_freshness_v2_deleted_file_is_stale(tmp_path) -> None:
    # A file removed after generation leaves no mtime of its own to catch,
    # but it does bump its parent directory's mtime — the walk must check
    # that too, or a deletion silently reads as fresh forever.
    head = _commit_repo(tmp_path)
    sub = tmp_path / "sub"
    sub.mkdir()
    extra = sub / "extra.py"
    extra.write_text("y = 2\n", encoding="utf-8")
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    generated_at = time.time() - 5.0
    _write_v2_fingerprint(gitnexus, head_sha=head, generated_at=generated_at)

    extra.unlink()

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert STALE_GITNEXUS_MARKER in detail


def test_freshness_snapshot_allows_dirty_tree_after_regeneration(tmp_path) -> None:
    # A dirty worktree relative to HEAD is not stale if this is the exact source
    # content GitNexus just analyzed.
    head = _commit_repo(tmp_path)
    (tmp_path / "f.py").write_text("x = 2\n", encoding="utf-8")
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    _write_snapshot_fingerprint(tmp_path, gitnexus, head_sha=head)

    assert _graph_freshness(tmp_path, gitnexus) == (False, None)


def test_freshness_snapshot_detects_same_mtime_content_edit(tmp_path) -> None:
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    _write_snapshot_fingerprint(tmp_path, gitnexus, head_sha=head)

    os.utime(tmp_path / "f.py", (100.0, 100.0))
    (tmp_path / "f.py").write_text("x = 2\n", encoding="utf-8")
    os.utime(tmp_path / "f.py", (100.0, 100.0))

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert STALE_GITNEXUS_MARKER in detail


def test_freshness_v2_tolerates_small_mtime_skew(tmp_path) -> None:
    # Simulate a system with clock drift.
    # We edit `f.py` after the graph was generated, but due to clock drift,
    # the filesystem mtime appears earlier than the process's `generated_at`.
    # Drift compensation uses the fingerprint file's mtime on the same FS, and
    # the edit is still correctly detected as stale.
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)

    # Write fingerprint file with generated_at=100.0, finished_at=100.0
    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps(
            {
                "head_sha": head,
                "generated_at": 100.0,
                "finished_at": 100.0,
            }
        ),
        encoding="utf-8",
    )
    # Set the fingerprint file mtime to 95.0, simulating a slow filesystem.
    os.utime(gitnexus / GITNEXUS_FINGERPRINT_FILE, (95.0, 95.0))

    # Set source mtime to 96.0: after generation on the filesystem clock,
    # but still less than the process's `generated_at` of 100.0.
    os.utime(tmp_path / "f.py", (96.0, 96.0))

    # Set containing directories' mtime to 94.0 (before generation) so the staleness
    # is correctly triggered by f.py itself rather than directory parent updates.
    os.utime(tmp_path, (94.0, 94.0))

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert "f.py" in detail


def test_freshness_v2_distrusts_implausible_duration(tmp_path) -> None:
    # finished_at < generated_at can't happen under normal generation (they're
    # recorded in that order within the same call), so this simulates a
    # corrupted/nonsensical fingerprint (e.g. a backward clock jump
    # mid-generation). The single-sample drift calibration would extrapolate
    # a bogus threshold from it; the duration clamp must instead fall back to
    # the flat _MTIME_SKEW_TOLERANCE_S tolerance so a real edit still gets
    # caught rather than silently trusted.
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)

    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps(
            {
                "head_sha": head,
                "generated_at": 100.0,
                "finished_at": 40.0,  # negative duration: implausible
            }
        ),
        encoding="utf-8",
    )
    os.utime(gitnexus / GITNEXUS_FINGERPRINT_FILE, (40.0, 40.0))

    # Without the clamp, the (uncorrected) calibration would extrapolate a
    # threshold of 100.0 here and miss this edit entirely. With the flat
    # tolerance (generated_at - 2.0 = 98.0), it's correctly caught.
    os.utime(tmp_path / "f.py", (99.0, 99.0))
    os.utime(tmp_path, (39.0, 39.0))

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is True
    assert "f.py" in detail


def test_freshness_v2_handles_pre_generation_mtimes_without_false_positives(
    tmp_path,
) -> None:
    # On the same clock-drifted system, a file modified BEFORE generation must NOT
    # be considered stale, even if its mtime is close to generation.
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)

    # Write fingerprint file with generated_at=100.0, finished_at=100.0
    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps(
            {
                "head_sha": head,
                "generated_at": 100.0,
                "finished_at": 100.0,
            }
        ),
        encoding="utf-8",
    )
    # Set the fingerprint file mtime to 95.0, simulating a slow filesystem.
    os.utime(gitnexus / GITNEXUS_FINGERPRINT_FILE, (95.0, 95.0))

    # Set source file's mtime to 94.0 (modified BEFORE generation on filesystem clock)
    os.utime(tmp_path / "f.py", (94.0, 94.0))

    # Set parent directory mtime to 94.0 as well (before generation)
    os.utime(tmp_path, (94.0, 94.0))

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    assert is_stale is False
    assert detail is None


def test_freshness_v2_truncated_mtime_pre_generation_false_positive(tmp_path) -> None:
    # Simulates a system with whole-second mtime truncation (e.g. FUSE/HFS+).
    # generated_at = 100.1 (float)
    # finished_at = 102.9 (float)
    # fingerprint mtime is truncated to whole seconds -> 102.0
    # A source file is edited BEFORE generation, at process clock 100.0.
    # Its mtime is truncated to whole seconds -> 100.0.
    # The current dynamic threshold logic pushes threshold to:
    #   threshold = 100.1 - (102.9 - 102.0) = 99.2.
    # Since 100.0 > 99.2, it is falsely flagged as stale under the old logic.
    # With truncation-aware logic, it is correctly handled and not flagged.
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)

    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps(
            {
                "head_sha": head,
                "generated_at": 100.1,
                "finished_at": 102.9,
            }
        ),
        encoding="utf-8",
    )
    # Set fingerprint's filesystem mtime to truncated whole seconds (102.0)
    os.utime(gitnexus / GITNEXUS_FINGERPRINT_FILE, (102.0, 102.0))

    # Set f.py's mtime to truncated 100.0 (process time 100.0, before 100.1 generation)
    os.utime(tmp_path / "f.py", (100.0, 100.0))
    os.utime(tmp_path, (100.0, 100.0))

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)

    # Under the old logic, this was True (falsely flagged as stale).
    # Under our new logic, it must be False.
    assert is_stale is False
    assert detail is None


def test_generate_ensure_regenerates_after_in_place_edit(tmp_path, monkeypatch) -> None:
    # End-to-end loop fix: with a v2 fingerprint and a post-generation edit,
    # the ensure default must regenerate instead of no-opping on PRESENT.
    _use_root(tmp_path, monkeypatch)
    head = _commit_repo(tmp_path)
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    source_mtime = (tmp_path / "f.py").stat().st_mtime
    _write_v2_fingerprint(gitnexus, head_sha=head, generated_at=source_mtime - 10)

    calls = []

    def fake_generate(d):
        calls.append(d)
        return DepgraphGenerationResult(True, 0, gitnexus, "done")

    # Real depgraph_status would try to load the stub lbug; freshness is what
    # this test exercises, so patch only the load, not the status pipeline.
    monkeypatch.setattr(depgraph_tool, "generate_depgraph", fake_generate)

    def fake_status(_override, project_root, _target_file):
        from topos.mcp.evaluation import _graph_freshness

        stale, detail = _graph_freshness(Path(project_root), gitnexus)
        return DepgraphStatus(
            state=(DepgraphState.STALE if stale else DepgraphState.PRESENT).value,
            gitnexus_dir=str(gitnexus),
            gitnexus_mtime=1.0,
            git_head_mtime=1.0,
            detail=detail,
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))

    assert calls, "ensure default must regenerate a working-tree-stale graph"
    assert r.ok is True
    assert r.generated is True
    assert r.state_before == DepgraphState.STALE


def _checkout_branch(root, name: str) -> None:
    subprocess.run(
        ["git", "-C", str(root), "checkout", "-b", name],
        check=True,
        capture_output=True,
    )


def _rename_branch(root, name: str) -> None:
    """Deterministically name the current branch, independent of the
    system's init.defaultBranch config (main vs master vs anything else)."""
    subprocess.run(
        ["git", "-C", str(root), "branch", "-m", name],
        check=True,
        capture_output=True,
    )


def _write_meta(store_dir: Path, *, branch: str) -> None:
    store_dir.mkdir(parents=True, exist_ok=True)
    (store_dir / "meta.json").write_text(
        json.dumps({"branch": branch, "lastCommit": "abc123"}), encoding="utf-8"
    )


def test_generate_runs_once_when_branch_not_indexed(tmp_path, monkeypatch) -> None:
    _use_root(tmp_path, monkeypatch)
    gitnexus = tmp_path / ".gitnexus"
    calls = []

    def fake_generate(d):
        calls.append(d)
        return DepgraphGenerationResult(True, 0, gitnexus, "done")

    def fake_status(_override, _project_root, _target_file):
        return DepgraphStatus(
            state=DepgraphState.BRANCH_NOT_INDEXED.value,
            gitnexus_dir=str(gitnexus),
            gitnexus_mtime=1.0,
            git_head_mtime=2.0,
            detail="no gitnexus store indexed for branch 'feature-x'",
        )

    monkeypatch.setattr(depgraph_tool, "depgraph_status", fake_status)
    monkeypatch.setattr(depgraph_tool, "generate_depgraph", fake_generate)

    r = _generate(topos_generate_depgraph(GenerateDepgraphInput()))

    assert calls == [tmp_path]
    assert r.ok is True
    assert r.generated is True
    assert r.state_before == DepgraphState.BRANCH_NOT_INDEXED
    assert r.agent_contract.next_tool == "topos_evaluate_file"


def test_depgraph_status_detects_branch_not_indexed_end_to_end(tmp_path) -> None:
    """Real depgraph_status() (no monkeypatching of the function itself):
    flat store belongs to 'main', current branch is 'feature-x', no
    branches/* store exists for it -> state must be branch_not_indexed, not
    a generic load_error."""
    from topos.mcp.evaluation import depgraph_status

    _commit_repo(tmp_path)
    _checkout_branch(tmp_path, "feature-x")
    gitnexus = _graph_dir(tmp_path, fingerprint=None)
    _write_meta(gitnexus, branch="main")

    status = depgraph_status(None, tmp_path, "f.py")
    assert status.state == "branch_not_indexed"
    assert "feature-x" in status.detail


def test_graph_freshness_uses_resolved_branch_store_not_stale_flat(tmp_path) -> None:
    """The fingerprint-desync bug: flat slot (main) carries a fingerprint that
    would read as STALE if consulted; the branches/feature-x store (current
    branch) carries a fingerprint that matches HEAD. _graph_freshness must
    resolve to the branch store, not the flat one."""
    head = _commit_repo(tmp_path)
    _checkout_branch(tmp_path, "feature-x")

    gitnexus = tmp_path / ".gitnexus"
    gitnexus.mkdir()
    flat_lbug = gitnexus / "lbug"
    flat_lbug.write_text("", encoding="utf-8")
    _write_meta(gitnexus, branch="main")
    (gitnexus / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps({"head_sha": "0" * 40}),
        encoding="utf-8",  # would read stale
    )

    branch_dir = gitnexus / "branches" / "feature-x-deadbeef"
    branch_dir.mkdir(parents=True)
    (branch_dir / "lbug").write_text("", encoding="utf-8")
    _write_meta(branch_dir, branch="feature-x")
    (branch_dir / GITNEXUS_FINGERPRINT_FILE).write_text(
        json.dumps({"head_sha": head}),
        encoding="utf-8",  # matches current HEAD
    )

    is_stale, detail = _graph_freshness(tmp_path, gitnexus)
    assert is_stale is False
    assert detail is None


def _write_json_lbug(store_dir: Path, *, file_path: str) -> None:
    lbug = store_dir / "lbug"
    lbug.mkdir(parents=True, exist_ok=True)
    (lbug / "graph.json").write_text(
        json.dumps(
            [
                {
                    "id": f"File:{file_path}",
                    "label": "File",
                    "properties": {"filePath": file_path},
                }
            ]
        ),
        encoding="utf-8",
    )


def test_dep_graph_for_cache_key_includes_branch_not_just_mtime(tmp_path) -> None:
    """Two branch-scoped stores sharing an identical mtime (coarse filesystem
    resolution, or a same-second CI run) must not collide in the cache --
    the branch itself has to be part of the key, not just the mtime."""
    clear_caches()
    _commit_repo(tmp_path)
    _rename_branch(tmp_path, "main")  # deterministic, regardless of system default
    gitnexus = tmp_path / ".gitnexus"

    main_dir = gitnexus / "branches" / "main-aaa"
    _write_json_lbug(main_dir, file_path="main.py")
    _write_meta(main_dir, branch="main")

    feature_dir = gitnexus / "branches" / "feature-x-bbb"
    _write_json_lbug(feature_dir, file_path="feature.py")
    _write_meta(feature_dir, branch="feature-x")

    same_time = 12345.0
    os.utime(main_dir / "lbug", (same_time, same_time))
    os.utime(feature_dir / "lbug", (same_time, same_time))

    _checkout_branch(tmp_path, "feature-x")
    on_feature = dep_graph_for(gitnexus, "feature.py")
    assert on_feature.get_node("File:feature.py") is not None
    assert on_feature.get_node("File:main.py") is None

    subprocess.run(
        ["git", "-C", str(tmp_path), "checkout", "main"],
        check=True,
        capture_output=True,
    )
    on_main = dep_graph_for(gitnexus, "main.py")
    assert on_main.get_node("File:main.py") is not None
    assert on_main.get_node("File:feature.py") is None
