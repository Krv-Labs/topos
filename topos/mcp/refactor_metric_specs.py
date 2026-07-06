"""Module-level metric target specs for refactor-target generation."""

from __future__ import annotations

from topos.evaluation.policies.calibration import COMPOSABLE, SIMPLE

MetricSpec = tuple[str, float | None, float | None, list[str]]

_METRIC_SPECS: tuple[MetricSpec, ...] = (
    (
        "ast.entropy",
        SIMPLE.min_entropy,
        SIMPLE.max_entropy,
        ["consolidate_boilerplate", "decompose_dense_logic"],
    ),
    ("mdg.fan_in", None, COMPOSABLE.max_fan_in, ["split_module"]),
    (
        "mdg.fan_out",
        None,
        COMPOSABLE.max_fan_out,
        ["reduce_fanout", "invert_dependency"],
    ),
    (
        "mdg.instability",
        COMPOSABLE.instability_low,
        COMPOSABLE.instability_high,
        ["rebalance_dependencies", "extract_boundary"],
    ),
)


def module_metric_specs(
    raw: dict[str, float],
) -> list[tuple[str, float, float, list[str]]]:
    """Return failing module-level metric specs as target tuples."""
    return [
        (metric, value, _threshold(value, low, high), operations)
        for metric, low, high, operations in _METRIC_SPECS
        if (value := raw.get(metric)) is not None and _outside(value, low, high)
    ]


def _outside(value: float, low: float | None, high: float | None) -> bool:
    return (low is not None and value < low) or (high is not None and value > high)


def _threshold(value: float, low: float | None, high: float | None) -> float:
    if low is not None and value < low:
        return low
    if high is not None:
        return high
    return value
