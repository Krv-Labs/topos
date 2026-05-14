"""
LRU caches for the Topos MCP server.

Two caches:
- ``dep_graph_for``: parsed ``DependencyGraph``, keyed by (gitnexus_dir,
  target_file, mtime). mtime invalidates automatically when gitnexus re-runs.
- ``baseline_result_for``: ``ClassificationResult`` for a file's current on-disk
  state, keyed by (filepath, content sha256, priority, gitnexus_mtime).
  Lets ``topos_assess_improvement`` skip re-evaluating the baseline across a
  loop of proposed variants.

Both caches are process-local; stdio servers are single-process so no
cross-process coordination is needed.
"""

from __future__ import annotations

import hashlib
from functools import lru_cache
from pathlib import Path

from topos.graphs.pdg.graph import DependencyGraph


def _gitnexus_mtime(gitnexus_dir: Path) -> float:
    """mtime of the gitnexus directory or its lbug file, for cache keying."""
    lbug = gitnexus_dir / "lbug"
    try:
        if lbug.exists():
            return lbug.stat().st_mtime
        return gitnexus_dir.stat().st_mtime
    except OSError:
        return 0.0


def _file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


@lru_cache(maxsize=32)
def _cached_dep_graph(
    gitnexus_dir_str: str, target_file: str, _mtime_for_key: float
) -> DependencyGraph:
    """Inner cached loader. ``_mtime_for_key`` is part of the cache key only."""
    del _mtime_for_key  # in key for invalidation; not used in body
    return DependencyGraph.from_gitnexus_dir(gitnexus_dir_str, target_file)


def dep_graph_for(gitnexus_dir: Path, target_file: str) -> DependencyGraph:
    """Return a cached ``DependencyGraph`` for the given gitnexus dir + file."""
    gitnexus_dir = Path(gitnexus_dir).resolve()
    mtime = _gitnexus_mtime(gitnexus_dir)
    return _cached_dep_graph(str(gitnexus_dir), target_file, mtime)


def baseline_key(
    filepath: Path,
    priority: str,
    gitnexus_dir: Path | None,
) -> tuple[str, str, str, float]:
    """Cache key for a file's current on-disk baseline evaluation."""
    gitnexus_mtime = _gitnexus_mtime(gitnexus_dir) if gitnexus_dir else 0.0
    return (str(filepath.resolve()), _file_sha256(filepath), priority, gitnexus_mtime)


def clear_caches() -> None:
    """Clear all caches. Primarily for tests."""
    _cached_dep_graph.cache_clear()
