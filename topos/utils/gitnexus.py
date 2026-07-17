"""Shared GitNexus dependency-graph generation.

A single place for the ``gitnexus analyze`` invocation so the CLI
(``topos depgraph generate``) and the MCP tool (``topos_generate_depgraph``)
stay in lockstep.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

GITNEXUS_CMD = "gitnexus"
_PNPM_INSTALL_CMD = "pnpm add -g gitnexus"
_NPM_INSTALL_CMD = "npm install -g gitnexus"


def gitnexus_install_command() -> str:
    """Preferred one-shot install command for the GitNexus CLI.

    Prefers ``pnpm`` when available, then ``npm``. Returns the pnpm form when
    neither package manager is on PATH so docs and error messages stay
    consistent with the preferred host workflow.
    """
    if shutil.which("pnpm") is not None:
        return _PNPM_INSTALL_CMD
    if shutil.which("npm") is not None:
        return _NPM_INSTALL_CMD
    return _PNPM_INSTALL_CMD


def gitnexus_install_hint() -> str:
    """User-facing message when the GitNexus CLI is missing."""
    preferred = gitnexus_install_command()
    alternate = (
        _NPM_INSTALL_CMD if preferred == _PNPM_INSTALL_CMD else _PNPM_INSTALL_CMD
    )
    return f"GitNexus not found. Install it with: {preferred}  # or: {alternate}"

# Topos-owned marker written inside ``.gitnexus`` recording what source snapshot
# the graph was built from. v1 markers carried only ``head_sha``; v2 added
# ``generated_at``/``finished_at``; current markers add a source content hash so
# freshness is not decided by fragile filesystem clocks.
GITNEXUS_FINGERPRINT_FILE = ".topos-fingerprint.json"

# GitNexus-owned files/dirs (not Topos-authored, unlike the fingerprint above).
# GitNexus writes the first-ever `gitnexus analyze` for a repo to the flat
# slot (`.gitnexus/lbug` + `.gitnexus/meta.json`). Re-running `analyze` on a
# *different* branch than the one recorded in the flat `meta.json` does not
# update the flat slot -- it creates `.gitnexus/branches/<branch>-<hash>/lbug`
# instead, each with its own `meta.json`.
GITNEXUS_META_FILE = "meta.json"
GITNEXUS_BRANCHES_DIR = "branches"

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
class LbugStoreMeta:
    """The subset of GitNexus's own ``meta.json`` Topos cares about."""

    branch: str | None
    last_commit: str | None


def _read_store_meta(store_dir: Path) -> LbugStoreMeta | None:
    """Best-effort parse of GitNexus's ``meta.json`` next to a store.

    Returns ``None`` on any read/parse error or a missing ``"branch"`` key --
    callers treat that identically to "no metadata, can't branch-match."
    """
    try:
        raw = json.loads((store_dir / GITNEXUS_META_FILE).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    branch = raw.get("branch")
    if not isinstance(branch, str):
        return None
    last_commit = raw.get("lastCommit")
    return LbugStoreMeta(
        branch=branch,
        last_commit=last_commit if isinstance(last_commit, str) else None,
    )


def current_git_branch(project_root: Path) -> str | None:
    """Current branch name, parsed from ``.git/HEAD`` without shelling out.

    Mirrors ``topos.mcp.evaluation._git_head_sha``'s no-subprocess approach
    exactly, since this is called on every dependency-graph read, not just at
    generate time. Returns ``None`` for a non-git dir, a detached HEAD (HEAD
    holds a raw SHA, not a ``ref:`` line), or a ref outside ``refs/heads/``
    (e.g. a tag checkout) -- all of which mean "no branch identity to match
    against," and callers fall back to the flat store unconditionally.
    """
    git_dir = project_root / ".git"
    try:
        head_text = (git_dir / "HEAD").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if not head_text.startswith("ref: "):
        return None  # detached HEAD: contents are a SHA, not a branch name
    ref = head_text.removeprefix("ref: ").strip()
    prefix = "refs/heads/"
    if not ref.startswith(prefix):
        return None
    return ref.removeprefix(prefix) or None


@dataclass(frozen=True)
class ResolvedLbugStore:
    """The ``lbug`` store to load for a given branch, or a "no match" report."""

    path: Path | None
    matched_branch: str | None
    available_branches: tuple[str, ...] = ()


def resolve_lbug_store(
    gitnexus_dir: Path, current_branch: str | None
) -> ResolvedLbugStore:
    """Pick the ``lbug`` store under ``gitnexus_dir`` matching *current_branch*.

    1. ``current_branch`` is ``None`` (no git / detached HEAD), or the flat
       store exists with no ``meta.json`` (a legacy, pre-branch-aware store)
       -> the flat slot unconditionally. This is what keeps the change fully
       backward compatible with every store GitNexus wrote before it started
       branch-scoping, and with detached-HEAD checkouts, which have no branch
       identity to match against.
    2. The flat store's ``meta.json`` records ``current_branch`` -> flat slot.
    3. Otherwise scan ``branches/*/meta.json`` for one recording
       ``current_branch`` (matched on the ``meta.json`` content, never on the
       opaque ``<hash>`` suffix GitNexus appends to the directory name -- we
       don't control how that hash is derived, and a branch name containing
       ``/`` may not round-trip through a directory-name-safe encoding).
    4. No match anywhere -> ``path=None``, ``available_branches`` populated
       (sorted) so the caller can build an actionable error message.
    """
    flat_lbug = gitnexus_dir / "lbug"
    flat_meta = _read_store_meta(gitnexus_dir)

    if current_branch is None or (flat_lbug.exists() and flat_meta is None):
        return ResolvedLbugStore(
            path=flat_lbug if flat_lbug.exists() else None,
            matched_branch=flat_meta.branch if flat_meta else None,
        )

    if flat_meta is not None and flat_meta.branch == current_branch:
        return ResolvedLbugStore(path=flat_lbug, matched_branch=current_branch)

    available: list[str] = [flat_meta.branch] if flat_meta is not None else []
    branches_dir = gitnexus_dir / GITNEXUS_BRANCHES_DIR
    if branches_dir.is_dir():
        for candidate in sorted(branches_dir.iterdir()):
            meta = _read_store_meta(candidate)
            if meta is None:
                continue
            available.append(meta.branch)
            if meta.branch == current_branch:
                lbug = candidate / "lbug"
                if lbug.exists():
                    return ResolvedLbugStore(path=lbug, matched_branch=current_branch)

    return ResolvedLbugStore(
        path=None,
        matched_branch=None,
        available_branches=tuple(sorted({b for b in available if b})),
    )


@dataclass(frozen=True)
class DepgraphGenerationResult:
    """Outcome of a ``gitnexus analyze`` run."""

    ok: bool
    returncode: int
    gitnexus_path: Path | None
    message: str


@dataclass(frozen=True)
class SourceFingerprint:
    """Stable content identity for source files seen by GitNexus/Topos."""

    content_hash: str
    file_count: int


def source_fingerprint(root: Path) -> SourceFingerprint:
    """Hash source-file paths and bytes under ``root`` using existing discovery."""
    from topos.graphs.ast.languages import LANGUAGE_FILE_SUFFIXES
    from topos.utils.discovery import iter_source_files

    root = root.resolve()
    suffixes = tuple(
        {suffix for group in LANGUAGE_FILE_SUFFIXES.values() for suffix in group}
    )
    digest = hashlib.sha256()
    count = 0
    files = sorted(
        iter_source_files(root, suffixes=suffixes),
        key=lambda path: path.relative_to(root).as_posix(),
    )
    for path in files:
        try:
            rel = path.relative_to(root).as_posix()
            stat = path.stat()
        except OSError:
            continue
        count += 1
        digest.update(rel.encode("utf-8", errors="surrogateescape"))
        digest.update(b"\0")
        digest.update(str(stat.st_size).encode("ascii"))
        digest.update(b"\0")
        try:
            with path.open("rb") as handle:
                for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                    digest.update(chunk)
        except OSError:
            continue
        digest.update(b"\0")
    return SourceFingerprint(content_hash=digest.hexdigest(), file_count=count)


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
        return DepgraphGenerationResult(False, 127, None, gitnexus_install_hint())

    start_time = time.time()
    source_snapshot = source_fingerprint(target_dir)
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
    branch = current_git_branch(target_dir)
    resolved = resolve_lbug_store(gitnexus_path, branch)
    fingerprint_dir = (
        resolved.path.parent if resolved.path is not None else gitnexus_path
    )
    _write_fingerprint(
        target_dir,
        fingerprint_dir,
        start_time=start_time,
        source_snapshot=source_snapshot,
    )
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


def _write_fingerprint(
    target_dir: Path,
    gitnexus_path: Path,
    start_time: float | None = None,
    source_snapshot: SourceFingerprint | None = None,
) -> None:
    """Record what the graph was built from (best-effort).

    Never raises: a write failure must not turn a successful generation into a
    failure.
    """
    sha = _head_sha(target_dir)
    if not isinstance(sha, str):
        sha = None
    # Best-effort: a read-only FS (OSError) or unexpected payload (Type/ValueError
    # from json) must never turn a successful generation into a failure.
    with contextlib.suppress(OSError, TypeError, ValueError):
        now = time.time()
        (gitnexus_path / GITNEXUS_FINGERPRINT_FILE).write_text(
            json.dumps(
                {
                    "head_sha": sha or None,
                    "generated_at": start_time or now,
                    "finished_at": now,
                    "source_hash": (
                        source_snapshot.content_hash if source_snapshot else None
                    ),
                    "source_file_count": (
                        source_snapshot.file_count if source_snapshot else None
                    ),
                }
            ),
            encoding="utf-8",
        )
