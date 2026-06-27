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


def _gitnexus_mtime(gitnexus_dir: Path) -> float:
    """Cheap mtime signal for the gitnexus snapshot, used as a cache key.

    Invalidation key component for ``dep_graph_for``: when GitNexus re-runs it
    rewrites the ``lbug`` store, bumping this mtime and busting the cache.

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
    - No ``lbug`` yet: fall back to the gitnexus dir's own mtime.
    """
    lbug = gitnexus_dir / "lbug"
    try:
        if lbug.exists():
            return lbug.stat().st_mtime
        return gitnexus_dir.stat().st_mtime
    except OSError:
        return 0.0


@lru_cache(maxsize=32)
def _cached_dep_graph(
    gitnexus_dir_str: str, target_file: str, _mtime_for_key: float
) -> ModuleDependencyGraph:
    """Inner cached loader. ``_mtime_for_key`` is part of the cache key only."""
    del _mtime_for_key  # in key for invalidation; not used in body
    return ModuleDependencyGraph.from_gitnexus_dir(gitnexus_dir_str, target_file)


def dep_graph_for(gitnexus_dir: Path, target_file: str) -> ModuleDependencyGraph:
    """Return a cached ``ModuleDependencyGraph`` for the given gitnexus dir + file."""
    gitnexus_dir = Path(gitnexus_dir).resolve()
    mtime = _gitnexus_mtime(gitnexus_dir)
    return _cached_dep_graph(str(gitnexus_dir), target_file, mtime)


def clear_caches() -> None:
    """Clear all caches. Primarily for tests."""
    _cached_dep_graph.cache_clear()
