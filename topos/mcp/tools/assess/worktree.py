"""
Worktree assessment tools (git integration).
"""

from __future__ import annotations

import subprocess
from pathlib import Path

from fastmcp.tools.base import ToolResult

from topos.utils.discovery import find_git_root

from ...schemas import AssessWorktreeChangeInput, resolve_priority
from ...security import resolve_within_root
from ...server import mcp
from .core import _assess_edit_in_place, _err_assessment, _READ_ONLY_ANN

def _git_show(
    repo_root: Path, ref: str, rel_path: str
) -> tuple[str | None, str | None]:
    """Read ``<ref>:<rel_path>`` from git, mirroring discovery.py's git idiom.

    Returns ``(source, None)`` or ``(None, blocked_by_code)``. ``git_unavailable``
    when git is not installed; ``baseline_ref_not_found`` when the ref or path
    does not exist at that revision (also covers timeouts/OS errors).
    """
    try:
        result = subprocess.run(
            ["git", "-C", str(repo_root), "show", f"{ref}:{rel_path}"],
            capture_output=True,
            timeout=5,
        )
    except FileNotFoundError:
        return None, "git_unavailable"
    except (OSError, subprocess.TimeoutExpired):
        return None, "baseline_ref_not_found"
    if result.returncode != 0:
        return None, "baseline_ref_not_found"
    return result.stdout.decode("utf-8", errors="replace"), None


@mcp.tool(
    name="topos_assess_worktree_change",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_worktree_change(params: AssessWorktreeChangeInput) -> ToolResult:
    """Assess an in-place edit against a git revision — the common refactor loop.

    Stateless: the baseline is read from git (``git show <baseline_ref>:<path>``,
    default ``HEAD``) and compared to the current working-tree file. No prior
    call required — edit the file, then ask "did it beat HEAD?". COMPOSABLE is
    scored when a ``.gitnexus`` dep graph is available.

    For untracked/new files or an uncommitted pre-edit baseline (which git
    cannot serve), use ``topos_begin_refactor`` + ``topos_assess_snapshot``.
    """
    priority, priority_source = resolve_priority(params.preferences)
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_assessment(
            priority, priority_source, (err or {}).get("error", "path error")
        )

    git_root = find_git_root(resolved)
    if git_root is None:
        return _err_assessment(
            priority,
            priority_source,
            f"Not inside a git repository: {resolved}",
            blocked_by="not_a_git_repo",
        )
    rel_path = resolved.relative_to(git_root).as_posix()
    baseline_src, git_err = _git_show(git_root, params.baseline_ref, rel_path)
    if git_err or baseline_src is None:
        return _err_assessment(
            priority,
            priority_source,
            f"Could not read `{rel_path}` at ref `{params.baseline_ref}`.",
            blocked_by=git_err or "baseline_ref_not_found",
        )

    prefs = params.preferences.to_preferences() if params.preferences else None
    return _assess_edit_in_place(
        baseline_src=baseline_src,
        resolved_path=resolved,
        gitnexus_dir_override=params.gitnexus_dir,
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        extra_warnings=[],
    )
