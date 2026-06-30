"""Map failing SIMPLE complexity gates to concrete source locations.

``topos_evaluate_file`` can report a failing ``ast.max_function_complexity``
without telling the agent *where* to edit. This module derives the offending
function spans from the same AST probe that produces the gate metric, so the
location and the metric never disagree.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.calibration import SIMPLE
from topos.functors.probes.ast.complexity import (
    FunctionComplexity,
    calculate_function_complexity_entries,
)

from .schemas import FunctionEntry


def function_entry_from_complexity(
    fc: FunctionComplexity, *, metric_source: str
) -> FunctionEntry:
    """Lift the probe dataclass into the MCP wire model."""
    return FunctionEntry(
        name=fc.name,
        line=fc.start_line,
        complexity=fc.complexity,
        qualified_name=fc.qualified_name,
        kind=fc.kind,
        start_line=fc.start_line,
        end_line=fc.end_line,
        metric_source=metric_source,
        includes_nested=fc.includes_nested,
    )


def _module_marker(metric_source: str, complexity: int) -> FunctionEntry:
    """Explicit 'not attributable to a function' marker for module-level gates."""
    return FunctionEntry(
        name="<module>",
        line=1,
        complexity=complexity,
        qualified_name="<module>",
        kind="module",
        start_line=1,
        metric_source=metric_source,
        includes_nested=True,
    )


def build_metric_locations(
    source: str, language: str, result: ClassificationResult
) -> dict[str, list[FunctionEntry]]:
    """Source locations for each failing SIMPLE complexity gate.

    - ``ast.max_function_complexity`` resolves to the offending functions
      (complexity above the per-function gate), sorted worst-first.
    - ``cfg.cyclomatic`` is a whole-module count, so it gets a ``kind='module'``
      marker rather than a misleading function span.
    """
    raw = result.raw_metrics
    locations: dict[str, list[FunctionEntry]] = {}

    max_func = raw.get("ast.max_function_complexity")
    if max_func is not None and max_func > SIMPLE.max_function_complexity:
        # The gate is the max over the same per-function probe, so a failing
        # gate always has at least one offending function; only attach the key
        # when we actually resolved spans (omit rather than emit an empty list).
        offending = _offending_functions(source, language)
        if offending:
            locations["ast.max_function_complexity"] = offending

    cyclomatic = raw.get("cfg.cyclomatic")
    if cyclomatic is not None and cyclomatic > SIMPLE.max_cyclomatic:
        locations["cfg.cyclomatic"] = [_module_marker("cfg", int(cyclomatic))]

    return locations


def _offending_functions(source: str, language: str) -> list[FunctionEntry]:
    morphism = ProgramMorphism(source=source, language=language)
    if morphism.ast is None or not morphism.is_valid:
        return []
    entries = [
        e
        for e in calculate_function_complexity_entries(morphism.ast)
        if e.complexity > SIMPLE.max_function_complexity
    ]
    entries.sort(key=lambda e: -e.complexity)
    return [function_entry_from_complexity(e, metric_source="ast") for e in entries]
