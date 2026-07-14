"""
LRU cache for the Topos MCP server.

``dep_graph_for``: parsed ``ModuleDependencyGraph``, keyed by (gitnexus_dir,
target_file, mtime). mtime invalidates automatically when gitnexus re-runs.

The cache is process-local; stdio servers are single-process so no
cross-process coordination is needed.

(Baseline source preservation across an edit-in-place loop lives in
``topos.mcp.snapshots`` — a content-addressed on-disk store — not here.)
"""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from topos.graphs.mdg.object import ModuleDependencyGraph
from topos.utils.gitnexus import current_git_branch, resolve_lbug_store


def _gitnexus_mtime(gitnexus_dir: Path, current_branch: str | None) -> float:
    """Cheap mtime signal for the gitnexus snapshot, used as a cache key.

    Invalidation key component for ``dep_graph_for``: when GitNexus re-runs it
    rewrites the ``lbug`` store, bumping this mtime and busting the cache.
    ``current_branch`` picks the *right* store to stat (see
    ``resolve_lbug_store``) — a repo with multiple branch-scoped stores must
    not report the flat slot's mtime while actually loading a
    ``branches/*`` store, or vice versa.

    Resolution by snapshot format (see ``ModuleDependencyGraph.from_gitnexus_dir``):
    - Binary LadybugDB (``lbug`` is a file, GitNexus >= 1.5): the file's mtime is
      exact — it changes whenever the store is rewritten.
    - Legacy JSON (``lbug`` is a directory): we use the directory's mtime. This is
      an APPROXIMATION — a directory's mtime changes when entries are added or
      removed but NOT when an existing ``*.json`` file is edited in place, so an
      in-place edit could produce a stale cache hit. Correctness-over-cleverness:
      a missed invalidation here is the one failure mode we must avoid, but the
      legacy JSON path is only produced by old GitNexus versions and snapshots are
      regenerated wholesale (dir mtime bumps) in practice. Full implementation if
      this ever bites: hash every ``lbug/*.json`` file (sha256 of the bytes)
      and fold the digest into the key instead of the dir mtime.
    - No matching store yet: fall back to the gitnexus dir's own mtime.
    """
    resolved = resolve_lbug_store(gitnexus_dir, current_branch)
    lbug = resolved.path
    try:
        if lbug is not None and lbug.exists():
            return lbug.stat().st_mtime
        return gitnexus_dir.stat().st_mtime
    except OSError:
        return 0.0


@lru_cache(maxsize=32)
def _cached_dep_graph(
    gitnexus_dir_str: str,
    target_file: str,
    current_branch: str | None,
    _mtime_for_key: float,
) -> ModuleDependencyGraph:
    """Inner cached loader. ``_mtime_for_key`` is part of the cache key only.

    ``current_branch`` is also part of the key (not just an input to the
    mtime computed by the caller): two branch-scoped stores could share an
    mtime on a coarse-resolution filesystem or a same-second CI run, and
    without the branch itself in the key that collision would silently
    serve the wrong cached graph even with a correct mtime source.
    """
    del _mtime_for_key  # in key for invalidation; not used in body
    return ModuleDependencyGraph.from_gitnexus_dir(gitnexus_dir_str, target_file)


def dep_graph_for(gitnexus_dir: Path, target_file: str) -> ModuleDependencyGraph:
    """Return a cached ``ModuleDependencyGraph`` for the given gitnexus dir + file."""
    gitnexus_dir = Path(gitnexus_dir).resolve()
    branch = current_git_branch(gitnexus_dir.parent)
    mtime = _gitnexus_mtime(gitnexus_dir, branch)
    return _cached_dep_graph(str(gitnexus_dir), target_file, branch, mtime)


def clear_caches() -> None:
    """Clear all caches. Primarily for tests."""
    _cached_dep_graph.cache_clear()
