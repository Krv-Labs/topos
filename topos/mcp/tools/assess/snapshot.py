"""
Snapshot assessment tools.
"""

from __future__ import annotations

import hashlib

from fastmcp.tools.base import ToolResult

from topos.evaluation.policies.base import Priority

from ...formatting import to_tool_result
from ...schemas import (
    AgentContract,
    AssessSnapshotInput,
    BeginRefactorInput,
    LatticeElement,
    PrioritySource,
    SnapshotResult,
    resolve_priority,
)
from ...security import read_safe_utf8_file, resolve_file_root, resolve_within_root
from ...server import mcp
from ...snapshots import now as snapshot_now
from ...snapshots import read_snapshot, write_snapshot
from .core import _READ_ONLY_ANN, _assess_edit_in_place, _err_assessment

_WRITE_ANN = {
    "title": "Topos Begin Refactor",
    "readOnlyHint": False,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="topos_begin_refactor",
    tags={"assess", "workflow"},
    annotations=_WRITE_ANN,
)
def topos_begin_refactor(params: BeginRefactorInput) -> ToolResult:
    """Capture a file's current source as a baseline snapshot before editing.

    Returns a ``snapshot_id``. Edit the file in place, then call
    ``topos_assess_snapshot(snapshot_id, filepath)`` to score the change — no
    need to re-send the baseline. Use this when the baseline is not a committed
    git revision (untracked/new files, or uncommitted prior edits); otherwise
    ``topos_assess_worktree_change`` needs no snapshot at all.
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_snapshot(params.filepath, (err or {}).get("error", "path error"))
    if not resolved.is_file():
        return _err_snapshot(params.filepath, f"Path is not a file: {resolved}")
    baseline_src, read_err = read_safe_utf8_file(resolved)
    if read_err or baseline_src is None:
        return _err_snapshot(
            params.filepath, (read_err or {}).get("error", "read error")
        )

    priority, _ = resolve_priority(params.preferences)
    meta = {
        "filepath": str(resolved),
        "priority": priority.value,
        "ranking": (
            [g.value for g in params.preferences.ranking]
            if params.preferences
            else None
        ),
        "target": (
            params.preferences.target.value
            if params.preferences and params.preferences.target
            else None
        ),
        "gitnexus_dir": params.gitnexus_dir,
    }
    created_at = snapshot_now()
    project_root = resolve_file_root()
    snapshot_id = write_snapshot(
        project_root, baseline_src, meta, created_at=created_at
    )
    model = SnapshotResult(
        snapshot_id=snapshot_id,
        filepath=str(resolved),
        baseline_hash=hashlib.sha256(baseline_src.encode("utf-8")).hexdigest(),
        created_at=created_at,
        agent_contract=AgentContract(
            next_tool="topos_assess_snapshot",
            next_actions=[
                "edit the file in place, then call topos_assess_snapshot with this "
                "snapshot_id"
            ],
        ),
    )
    return to_tool_result(model, _render_snapshot_md(model))


@mcp.tool(
    name="topos_assess_snapshot",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_snapshot(params: AssessSnapshotInput) -> ToolResult:
    """Assess the current file against a baseline captured by topos_begin_refactor.

    Loads the stored baseline by ``snapshot_id`` and compares it to the current
    on-disk file, with the same status semantics as ``topos_assess_improvement``
    (and COMPOSABLE scored when a dep graph is available). A missing or expired
    snapshot is reported via ``blocked_by`` (``snapshot_not_found`` /
    ``snapshot_stale``).
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            (err or {}).get("error", "path error"),
        )

    project_root = resolve_file_root()
    load = read_snapshot(project_root, params.snapshot_id, at=snapshot_now())
    if load.blocked_by or load.meta is None or load.baseline_src is None:
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            f"Snapshot `{params.snapshot_id}` is unavailable.",
            blocked_by=load.blocked_by or "snapshot_not_found",
        )
    if load.meta.get("filepath") != str(resolved):
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            (
                f"Snapshot was taken from `{load.meta.get('filepath')}`, not "
                f"`{resolved}`."
            ),
            blocked_by="snapshot_stale",
        )

    priority, priority_source, prefs = _priority_from_meta(load.meta)
    return _assess_edit_in_place(
        baseline_src=load.baseline_src,
        resolved_path=resolved,
        gitnexus_dir_override=load.meta.get("gitnexus_dir"),
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        extra_warnings=[],
    )


def _priority_from_meta(meta: dict):
    """Reconstruct (priority, priority_source, preferences) from a snapshot sidecar."""
    ranking = meta.get("ranking")
    if not ranking:
        return (
            Priority(meta.get("priority", Priority.SIMPLE.value)),
            (PrioritySource.DEFAULT),
            None,
        )
    from topos.evaluation.preferences import Generator

    from ..schemas import UserPreferencesInput

    target = meta.get("target")
    prefs_input = UserPreferencesInput(
        ranking=[Generator(r) for r in ranking],
        target=LatticeElement(target) if target else None,
    )
    return (
        prefs_input.to_priority(),
        PrioritySource.PREFERENCES,
        prefs_input.to_preferences(),
    )


def _err_snapshot(filepath: str, msg: str) -> ToolResult:
    model = SnapshotResult(
        snapshot_id="",
        filepath=filepath,
        baseline_hash="",
        created_at=0.0,
        agent_contract=AgentContract(
            blocked_by=["snapshot_error"], risk_flags=["snapshot_error"]
        ),
        error=msg,
    )
    return to_tool_result(model, _render_snapshot_md(model))


def _render_snapshot_md(r: SnapshotResult) -> str:
    if r.error:
        return f"**Error:** {r.error}"
    lines = [
        f"**Snapshot captured:** `{r.snapshot_id}`",
        f"**File:** `{r.filepath}`",
        "",
        "Edit the file in place, then call "
        f'`topos_assess_snapshot(snapshot_id="{r.snapshot_id}", '
        f'filepath="{r.filepath}")`.',
    ]
    return "\\n".join(lines)
