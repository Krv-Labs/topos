"""
Policy calibration — central hub for evaluation gates and scoring constants.

Edit the module-level singletons (``SIMPLE``, ``COMPOSABLE``, ``SECURE``,
``COVERAGE``, ``CLONE``) and ``SCORE_FLOORS`` when updating from experimental
data. All policy translators import from this module; nothing else should
define pass/fail or normalization numbers.

Sections
--------
**Raw-metric gates** — drive ``ScoredDecision.achieved`` (AND semantics).
Each ``Φᵢ`` compares probe values against these fields; they are the decisive
pass/fail criteria for the three quality generators in Ω.

**Normalization caps/scales** — map raw metrics to ``[0, 1]`` quality scores
for reporting and multi-file aggregation. They do **not** gate ``achieved``.

**Score floors** — alternate path via
:func:`~topos.evaluation.policies.base.meet_satisfied` and multi-file
:class:`~topos.evaluation.characteristic_morphism.CharacteristicMorphism`
meets. Live ``Φᵢ`` translators do not use these for ``achieved``.

**Auxiliary** — clone detection and declaration-coverage defaults (outside Ω).

Calibration provenance: PyPI corpus ECDF calibration (June 2026).
See topos-leaderboard/CALIBRATION_REPORT.md and calibration.json.
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.evaluation.preferences import Generator


@dataclass(frozen=True)
class SimplePolicyThresholds:
    """Φ_SIMPLE gates and normalization."""

    # Gates (achieved)
    max_cyclomatic: float = 15.0
    max_function_complexity: float = 10.0
    min_entropy: float = 0.2
    max_entropy: float = 0.8
    # Normalization (score only)
    max_cyclomatic_cap: float = 40.0
    max_function_complexity_cap: float = 20.0
    entropy_ideal: float = 0.5


@dataclass(frozen=True)
class ComposablePolicyThresholds:
    """Φ_COMPOSABLE gates and normalization."""

    # Gates (achieved)
    instability_low: float = 0.3
    instability_high: float = 0.7
    max_fan_in: float = 15.0
    max_fan_out: float = 15.0
    # Entrypoint carve-out: import/export-only entrypoint modules with zero
    # fan-in may sit at or above this instability without failing the gate.
    entrypoint_instability_min: float = 0.95
    # Normalization (score only)
    max_fan_in_cap: float = 40.0
    max_fan_out_cap: float = 40.0


@dataclass(frozen=True)
class SecurePolicyThresholds:
    """Φ_SECURE gates and normalization."""

    # Gates (achieved) — strict zero-tolerance security
    max_dangerous_calls: float = 0.0
    max_taint_flows: float = 0.0
    # Normalization (score only) — exponential decay scales
    danger_scale: float = 3.0
    taint_scale: float = 3.0


@dataclass(frozen=True)
class CoveragePolicyThresholds:
    """Structural test-coverage policy (outside Ω)."""

    declaration_recall: float = 0.5
    strong_offset: float = 0.25  # "strong" band above gate
    partial_factor: float = 0.5  # "partial" band = gate × this


@dataclass(frozen=True)
class ClonePolicyThresholds:
    """Pairwise clone detection (outside Ω)."""

    max_normalized_distance: float = 0.1


# Module-level singletons — edit these after experiments.
SIMPLE = SimplePolicyThresholds()
COMPOSABLE = ComposablePolicyThresholds()
SECURE = SecurePolicyThresholds()
COVERAGE = CoveragePolicyThresholds()
CLONE = ClonePolicyThresholds()

# Score-floor alternate path (meet_satisfied + multi-file CharacteristicMorphism).
SCORE_FLOORS: dict[Generator, float] = {
    Generator.SIMPLE: 0.40,
    Generator.COMPOSABLE: 0.80,
    Generator.SECURE: 1.00,
}
