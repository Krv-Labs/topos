"""CPG taint probes — thin wrappers over the Rust engine."""

from __future__ import annotations

from topos.topos_functors import taint_flow_paths_py as taint_flow_paths
from topos.topos_functors import taint_sources_for_language

TAINT_SOURCES: dict[str, set[str]] = {
    "python": taint_sources_for_language("python"),
    "javascript": taint_sources_for_language("javascript"),
    "typescript": taint_sources_for_language("typescript"),
    "rust": taint_sources_for_language("rust"),
    "cpp": taint_sources_for_language("cpp"),
    "go": taint_sources_for_language("go"),
}

__all__ = ["TAINT_SOURCES", "taint_flow_paths", "taint_sources_for_language"]
