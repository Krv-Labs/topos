"""
Canonical gate specs — the single structural source of truth for pass/fail.

``calibration`` owns the *numbers*; this module owns the *structure*: which
raw metric belongs to which pillar, which side(s) of a band it is gated on,
which exemptions apply, what a failure means in prose, and which refactor
operations address it. Every consumer of a gate comparison — the Φᵢ scorers,
the suggestion engine, and MCP refactor targets — evaluates gates through
:func:`evaluate_gates` so their verdicts can never diverge.

Score *shaping* (normalization caps, quality curves) deliberately stays in
the scorers; only the decisive pass/fail comparisons live here.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass
from enum import Enum

from topos.evaluation.policies.calibration import COMPOSABLE, SECURE, SIMPLE


class GateOutcome(Enum):
    """How a metric fared against its gate band."""

    PASS = "pass"
    FAIL_LOW = "fail_low"  # value < low bound
    FAIL_HIGH = "fail_high"  # value > high bound
    EXEMPT_LOW = "exempt_low"  # below low, but the exemption predicate held
    EXEMPT_HIGH = "exempt_high"  # above high, but the exemption predicate held


_PASSING = frozenset(
    {GateOutcome.PASS, GateOutcome.EXEMPT_LOW, GateOutcome.EXEMPT_HIGH}
)
_LOW_SIDE = frozenset({GateOutcome.FAIL_LOW, GateOutcome.EXEMPT_LOW})


@dataclass(frozen=True)
class GateContext:
    """Everything an exemption predicate may read."""

    value: float
    metrics: Mapping[str, float]
    is_entrypoint_module: bool
    is_stable_leaf_module: bool = False
    # Raw Martin instability, threaded separately from `metrics` because
    # `mdg.instability` is deliberately absent from the gated metrics dict
    # whenever `mdg.main_sequence_distance` is active (see
    # topos.evaluation.policies.composable.score_coupling) — re-adding it
    # under its own key would re-trigger the (now-superseded)
    # `mdg.instability` GateSpec for the same file.
    instability: float | None = None


@dataclass(frozen=True)
class GateSpec:
    """One raw-metric gate: band, pillar, exemption, remedy, and prose."""

    metric: str
    pillar: str  # "simple" | "composable" | "secure"
    low: float | None  # inclusive lower bound; None = unbounded below
    high: float | None  # inclusive upper bound; None = unbounded above
    granularity: str  # "function" | "module"
    interpret: Callable[[float, GateOutcome], str]
    exempt: Callable[[GateContext], bool] | None = None
    operations_low: tuple[str, ...] = ()
    operations_high: tuple[str, ...] = ()


@dataclass(frozen=True)
class GateResult:
    """A spec applied to a measured value."""

    spec: GateSpec
    value: float
    outcome: GateOutcome

    @property
    def passed(self) -> bool:
        """True for PASS and for exempted failures (the gate is satisfied)."""
        return self.outcome in _PASSING

    @property
    def threshold(self) -> float | None:
        """The bound on the violated side, or None when in band."""
        if self.outcome is GateOutcome.PASS:
            return None
        return self.spec.low if self.outcome in _LOW_SIDE else self.spec.high

    @property
    def operations(self) -> tuple[str, ...]:
        """Refactor operations for the violated side (empty when in band)."""
        if self.outcome is GateOutcome.PASS:
            return ()
        return (
            self.spec.operations_low
            if self.outcome in _LOW_SIDE
            else self.spec.operations_high
        )

    @property
    def interpretation(self) -> str:
        return self.spec.interpret(self.value, self.outcome)


# ---------------------------------------------------------------------------
# Exemption predicates (the scorer carve-outs, expressed once)
# ---------------------------------------------------------------------------


def _entropy_entrypoint_exempt(ctx: GateContext) -> bool:
    """Import/export-only entrypoint modules may sit below the entropy floor."""
    return ctx.is_entrypoint_module and ctx.value < SIMPLE.min_entropy


def _instability_entrypoint_exempt(ctx: GateContext) -> bool:
    """Entrypoint modules with zero fan-in may sit at maximal instability.

    ``metrics.get(...) == 0.0`` deliberately fails when fan-in is unmeasured:
    an absent metric never grants the exemption (mirrors the original
    ``fan_in == 0.0`` against a possibly-None argument).
    """
    return (
        ctx.is_entrypoint_module
        and ctx.value >= COMPOSABLE.entrypoint_instability_min
        and ctx.metrics.get("mdg.fan_in") == 0.0
    )


def _distance_stable_leaf_exempt(ctx: GateContext) -> bool:
    """Frozen, declarations-only leaf modules may sit at maximal distance
    from the main sequence — Martin's accepted "Zone of Pain" exception for
    foundation/utility code (constants, error types) that is stable *and*
    concrete by design, not because it's poorly layered.
    """
    return (
        ctx.is_stable_leaf_module
        and ctx.instability is not None
        and ctx.instability <= COMPOSABLE.stable_leaf_instability_max
    )


# ---------------------------------------------------------------------------
# Interpretation renderers (canonical prose, byte-identical to the legacy
# per-scorer helpers)
# ---------------------------------------------------------------------------


def _interpret_cyclomatic(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return (
            f"cyclomatic complexity ({value:.0f}) within threshold "
            f"(<= {SIMPLE.max_cyclomatic})"
        )
    return (
        f"cyclomatic complexity ({value:.0f}) exceeds threshold "
        f"(> {SIMPLE.max_cyclomatic})"
    )


def _interpret_max_func(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return (
            f"max function complexity ({value:.0f}) within threshold "
            f"(<= {SIMPLE.max_function_complexity})"
        )
    return (
        f"max function complexity ({value:.0f}) exceeds threshold "
        f"(> {SIMPLE.max_function_complexity})"
    )


def _interpret_entropy(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return (
            f"entropy ({value:.2f}) within structured range "
            f"[{SIMPLE.min_entropy}, {SIMPLE.max_entropy}]"
        )
    if outcome is GateOutcome.EXEMPT_LOW:
        return (
            f"entropy ({value:.2f}) is low, but tolerated for "
            "import/export-only entrypoint modules"
        )
    if outcome is GateOutcome.FAIL_LOW:
        return f"entropy ({value:.2f}) is too low; code may be repetitive or trivial"
    return f"entropy ({value:.2f}) is too high; code may be unstructured"


def _interpret_instability(value: float, outcome: GateOutcome) -> str:
    low, high = COMPOSABLE.instability_low, COMPOSABLE.instability_high
    if outcome is GateOutcome.PASS:
        return f"instability ({value:.2f}) within balanced range [{low}, {high}]"
    if outcome is GateOutcome.FAIL_LOW:
        return f"instability ({value:.2f}) is too low (module is too stable)"
    if outcome is GateOutcome.EXEMPT_HIGH:
        return (
            f"instability ({value:.2f}) is high, but tolerated for "
            "import/export-only entrypoint modules"
        )
    return f"instability ({value:.2f}) is too high (module depends on too many things)"


def _interpret_main_sequence_distance(value: float, outcome: GateOutcome) -> str:
    max_d = COMPOSABLE.main_sequence_distance_max
    if outcome is GateOutcome.PASS:
        return (
            f"main-sequence distance ({value:.2f}) within tolerance "
            f"(<= {max_d}) — instability and abstractness are balanced"
        )
    if outcome is GateOutcome.EXEMPT_HIGH:
        return (
            f"main-sequence distance ({value:.2f}) is high, but tolerated "
            "for frozen, declarations-only leaf modules"
        )
    return (
        f"main-sequence distance ({value:.2f}) exceeds threshold (> {max_d}) "
        "— module is too concrete-and-stable (rigid) or too abstract-and-"
        "unstable (speculative) for its role"
    )


def _interpret_fan_in(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return f"fan-in ({value:.0f}) within threshold (<= {COMPOSABLE.max_fan_in})"
    return f"fan-in ({value:.0f}) exceeds threshold (> {COMPOSABLE.max_fan_in})"


def _interpret_fan_out(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return f"fan-out ({value:.0f}) within threshold (<= {COMPOSABLE.max_fan_out})"
    return f"fan-out ({value:.0f}) exceeds threshold (> {COMPOSABLE.max_fan_out})"


def _interpret_danger(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return (
            f"no reachable dangerous-API calls "
            f"({value:.0f} <= {SECURE.max_dangerous_calls})"
        )
    return (
        f"{int(value)} dangerous-API call site(s) exceeds threshold "
        f"({SECURE.max_dangerous_calls})"
    )


def _interpret_taint(value: float, outcome: GateOutcome) -> str:
    if outcome is GateOutcome.PASS:
        return f"no source→sink taint paths ({value:.0f} <= {SECURE.max_taint_flows})"
    return (
        f"{int(value)} taint flow path(s) exceeds threshold ({SECURE.max_taint_flows})"
    )


# ---------------------------------------------------------------------------
# The registry — ordered to match the scorers' interpretation insertion order
# ---------------------------------------------------------------------------

GATE_SPECS: tuple[GateSpec, ...] = (
    GateSpec(
        metric="cfg.cyclomatic",
        pillar="simple",
        low=None,
        high=SIMPLE.max_cyclomatic,
        granularity="function",
        interpret=_interpret_cyclomatic,
        operations_high=("extract_helper", "split_decision_logic"),
    ),
    GateSpec(
        metric="ast.entropy",
        pillar="simple",
        low=SIMPLE.min_entropy,
        high=SIMPLE.max_entropy,
        granularity="module",
        interpret=_interpret_entropy,
        exempt=_entropy_entrypoint_exempt,
        operations_low=("consolidate_boilerplate",),
        operations_high=("decompose_dense_logic",),
    ),
    GateSpec(
        metric="ast.max_function_complexity",
        pillar="simple",
        low=None,
        high=SIMPLE.max_function_complexity,
        granularity="function",
        interpret=_interpret_max_func,
        operations_high=("extract_helper", "split_decision_logic"),
    ),
    GateSpec(
        metric="mdg.instability",
        pillar="composable",
        low=COMPOSABLE.instability_low,
        high=COMPOSABLE.instability_high,
        granularity="module",
        interpret=_interpret_instability,
        exempt=_instability_entrypoint_exempt,
        operations_low=("rebalance_dependencies", "extract_boundary"),
        operations_high=("rebalance_dependencies", "extract_boundary"),
    ),
    GateSpec(
        metric="mdg.main_sequence_distance",
        pillar="composable",
        low=None,
        high=COMPOSABLE.main_sequence_distance_max,
        granularity="module",
        interpret=_interpret_main_sequence_distance,
        exempt=_distance_stable_leaf_exempt,
        operations_high=("rebalance_dependencies", "extract_boundary"),
    ),
    GateSpec(
        metric="mdg.fan_in",
        pillar="composable",
        low=None,
        high=COMPOSABLE.max_fan_in,
        granularity="module",
        interpret=_interpret_fan_in,
        operations_high=("split_module",),
    ),
    GateSpec(
        metric="mdg.fan_out",
        pillar="composable",
        low=None,
        high=COMPOSABLE.max_fan_out,
        granularity="module",
        interpret=_interpret_fan_out,
        operations_high=("reduce_fanout", "invert_dependency"),
    ),
    GateSpec(
        metric="cpg.dangerous_calls",
        pillar="secure",
        low=None,
        high=SECURE.max_dangerous_calls,
        granularity="module",
        interpret=_interpret_danger,
    ),
    GateSpec(
        metric="cpg.taint_flows",
        pillar="secure",
        low=None,
        high=SECURE.max_taint_flows,
        granularity="module",
        interpret=_interpret_taint,
    ),
)

GATES_BY_METRIC: dict[str, GateSpec] = {spec.metric: spec for spec in GATE_SPECS}

# Metric-key namespacing shared with the agent-contract/pillar layers.
PILLAR_METRIC_PREFIXES: dict[str, tuple[str, ...]] = {
    "simple": ("cfg.", "ast."),
    "composable": ("mdg.",),
    "secure": ("cpg.",),
}


def pillar_for_metric(metric: str) -> str:
    """Map a namespaced raw-metric key to its pillar (default 'simple')."""
    for pillar, prefixes in PILLAR_METRIC_PREFIXES.items():
        if metric.startswith(prefixes):
            return pillar
    return "simple"


def evaluate_gates(
    metrics: Mapping[str, float],
    *,
    pillar: str | None = None,
    is_entrypoint_module: bool = False,
    is_stable_leaf_module: bool = False,
    instability: float | None = None,
) -> list[GateResult]:
    """Apply every (optionally pillar-filtered) spec whose metric is present."""
    results: list[GateResult] = []
    for spec in GATE_SPECS:
        if pillar is not None and spec.pillar != pillar:
            continue
        value = metrics.get(spec.metric)
        if value is None:
            continue
        results.append(
            GateResult(
                spec=spec,
                value=value,
                outcome=_classify(
                    spec,
                    value,
                    metrics,
                    is_entrypoint_module,
                    is_stable_leaf_module,
                    instability,
                ),
            )
        )
    return results


def interpret_metric(metric: str, value: float) -> str:
    """Canonical prose for a single metric value (no exemption context)."""
    spec = GATES_BY_METRIC[metric]
    return spec.interpret(value, _classify(spec, value, {}, False, False, None))


def _classify(
    spec: GateSpec,
    value: float,
    metrics: Mapping[str, float],
    is_entrypoint_module: bool,
    is_stable_leaf_module: bool = False,
    instability: float | None = None,
) -> GateOutcome:
    if spec.low is not None and value < spec.low:
        fail, exempt = GateOutcome.FAIL_LOW, GateOutcome.EXEMPT_LOW
    elif spec.high is not None and value > spec.high:
        fail, exempt = GateOutcome.FAIL_HIGH, GateOutcome.EXEMPT_HIGH
    else:
        return GateOutcome.PASS
    ctx = GateContext(
        value, metrics, is_entrypoint_module, is_stable_leaf_module, instability
    )
    if spec.exempt is not None and spec.exempt(ctx):
        return exempt
    return fail
