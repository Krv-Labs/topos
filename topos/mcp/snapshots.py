"""
Content-addressed snapshot store for the Topos MCP refactor loop.

Lets an agent capture a file's baseline source *before* an in-place edit, then
assess the edited file against that baseline without re-sending the source
(``topos_begin_refactor`` → edit → ``topos_assess_snapshot``).

Two deliberate design choices:

- **Outside the working tree.** This server runs against arbitrary user repos.
  A store under the repo would pollute ``git status``, risk accidental commits,
  and fight ``topos_assess_worktree_change``'s own ``git show HEAD`` baseline.
  So the store lives under the system temp dir (honoring ``$TMPDIR`` — on macOS
  a per-user private dir, safer than world-writable ``/tmp``), namespaced per
  project root. Override with ``TOPOS_SNAPSHOT_DIR``.
- **On disk, content-addressed — not an in-process dict.** Snapshots survive a
  stdio-server restart, and the ``snapshot_id`` is a self-contained content hash
  rather than an opaque server handle (stdio MCP servers should avoid
  server-held handle state: it dies with the subprocess and is not shared across
  clients). The pure baseline hash is also recorded in the sidecar.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import re
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

# A snapshot_id is a sha256 hex digest (of filepath + content). Validate it
# before any path join so a caller-supplied id can never traverse out of the
# store (the store sits outside FILE_ACCESS_ROOT, so this is the path guard).
_SNAPSHOT_ID_RE = re.compile(r"^[0-9a-f]{64}$")

# Reject snapshots past this age on read, and sweep them on write. Refactor
# loops run for minutes to hours; a day is generous. OS temp cleanup is the
# backstop for anything the sweep misses.
_TTL_SECONDS = 24 * 60 * 60


@dataclass(frozen=True)
class SnapshotLoad:
    """Outcome of ``read_snapshot``.

    ``blocked_by`` is ``None`` on success and otherwise one of the contract
    codes ``snapshot_not_found`` / ``snapshot_stale`` so the tool layer can
    surface it directly in the ``AgentContract``.
    """

    baseline_src: str | None = None
    meta: dict | None = None
    blocked_by: str | None = None


def now() -> float:
    """Current wall-clock time (indirection keeps tool bodies easy to fake)."""
    return time.time()


def _store_root(project_root: Path) -> Path:
    """Per-project snapshot directory, outside the working tree.

    Honors ``$TOPOS_SNAPSHOT_DIR``; else ``<tempdir>/topos-snapshots/<root-hash>``.
    """
    override = os.getenv("TOPOS_SNAPSHOT_DIR")
    base = (
        Path(override).expanduser()
        if override
        else Path(tempfile.gettempdir()) / "topos-snapshots"
    )
    key = hashlib.sha256(str(project_root.resolve()).encode("utf-8")).hexdigest()[:16]
    return base / key


def _sweep_expired(root: Path, at: float) -> None:
    """Best-effort eviction of blobs/sidecars past the TTL.

    PONYTAIL: a linear scan on every write is fine for the handful of snapshots
    a refactor loop produces. If a store ever holds thousands, replace with a
    size-capped LRU index rather than statting every file.
    """
    try:
        entries = list(root.iterdir())
    except OSError:
        return
    for entry in entries:
        try:
            if at - entry.stat().st_mtime > _TTL_SECONDS:
                entry.unlink()
        except OSError:
            continue


def _harden(path: Path, mode: int) -> None:
    """Best-effort private permissions; the 0700 store dir is the real guard.

    Suppresses OSError on non-POSIX filesystems (e.g. Windows) — there the
    directory traversal bit on the 0700 store dir is what protects the blobs.
    """
    with contextlib.suppress(OSError):
        os.chmod(path, mode)


def write_snapshot(
    project_root: Path, baseline_src: str, meta: dict, *, created_at: float
) -> str:
    """Persist a baseline and return its ``snapshot_id``.

    The id is keyed by ``(filepath, content)`` — NOT content alone — so two
    files with identical baseline source (empty ``__init__.py``, copied
    scaffolds) get distinct handles instead of clobbering one shared sidecar.
    The pure content hash lives in the sidecar as ``baseline_hash``. The sidecar
    also carries ``filepath``/priority/preferences/``gitnexus_dir`` so
    ``topos_assess_snapshot`` stays a clean 2-arg call.

    On shared temp dirs (Linux ``/tmp``) the store would otherwise inherit the
    process umask, leaving baseline source other-readable; the store dir is
    pinned to 0700 and files to 0600.
    """
    baseline_bytes = baseline_src.encode("utf-8")
    baseline_hash = hashlib.sha256(baseline_bytes).hexdigest()
    snapshot_id = hashlib.sha256(
        f"{meta.get('filepath', '')}\0{baseline_hash}".encode()
    ).hexdigest()
    root = _store_root(project_root)
    root.mkdir(parents=True, exist_ok=True)
    _harden(root, 0o700)
    _sweep_expired(root, created_at)
    blob = root / f"{snapshot_id}.blob"
    if not blob.exists():  # dedup repeated captures of the same file+content
        blob.write_bytes(baseline_bytes)
        _harden(blob, 0o600)
    sidecar = root / f"{snapshot_id}.json"
    sidecar.write_text(
        json.dumps({**meta, "baseline_hash": baseline_hash, "created_at": created_at}),
        encoding="utf-8",
    )
    _harden(sidecar, 0o600)
    return snapshot_id


def read_snapshot(project_root: Path, snapshot_id: str, *, at: float) -> SnapshotLoad:
    """Load a baseline by ``snapshot_id``.

    Returns ``snapshot_not_found`` when the id is malformed or the blob/sidecar
    is missing, and ``snapshot_stale`` when the snapshot is past its TTL. The
    tool layer additionally treats a filepath mismatch as ``snapshot_stale``.
    """
    if not _SNAPSHOT_ID_RE.match(snapshot_id):
        return SnapshotLoad(blocked_by="snapshot_not_found")
    root = _store_root(project_root)
    try:
        baseline_src = (root / f"{snapshot_id}.blob").read_text(encoding="utf-8")
        meta = json.loads((root / f"{snapshot_id}.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return SnapshotLoad(blocked_by="snapshot_not_found")
    if at - meta.get("created_at", 0) > _TTL_SECONDS:
        return SnapshotLoad(blocked_by="snapshot_stale")
    return SnapshotLoad(baseline_src=baseline_src, meta=meta)
