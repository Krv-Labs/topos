"""
Shared scoring infrastructure for the policy translators Φᵢ : ℝ → Ω.

Following the math spec (§3 "Policy Translation"), each quality generator
gᵢ ∈ G_qual has an associated policy translator Φᵢ that maps real-valued
probe outputs (cyclomatic complexity, Martin instability, taint-flow
counts, …) into the truth-value carrier of Ω.

This module defines the shared types used by every Φᵢ:

- ``Priority``       — the manager's strict total order on G_qual, lifted
                       from ``README.md``.
- ``WeightProfile``  — per-generator intra-dimension metric weights.
- ``ScoredDecision`` — the output of a single Φᵢ.

There is exactly one ``Φᵢ`` per generator:

    Φ_SIMPLE      ↦ topos/evaluation/policies/simple.py::score_simple
    Φ_COMPOSABLE  ↦ topos/evaluation/policies/coupling.py::score_coupling
    Φ_SECURE      ↦ topos/evaluation/policies/secure.py::score_secure
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum


class Priority(StrEnum):
    """
    The manager's strict total order on the quality generators G_qual.

    Selecting a priority *does not* change the lattice — the three
    generators remain pairwise incomparable.  What it does is shift metric
    weights within each generator's policy translator Φᵢ, plus determine
    the order in which verdicts are walked during target-relaxation (see
    the bit-table in ``README.md``).

    Members align 1:1 with the generator set ``G_qual``:
        BALANCED:    Equal weights across all dimensions (default).
        SIMPLE:      Upweights the SIMPLE generator's metrics.
        COMPOSABLE:  Upweights the COMPOSABLE generator's metrics.
        SECURE:      Upweights the SECURE generator's metrics.
    """

    BALANCED = "balanced"
    SIMPLE = "simple"
    COMPOSABLE = "composable"
    SECURE = "secure"


@dataclass(frozen=True)
class WeightProfile:
    """
    Per-generator metric weights for a given Priority.

    Each weight controls the linear combination *within* one Φᵢ between
    its two principal metrics.  The two weights inside a single
    ``WeightProfile`` are independent across dimensions — they do not
    sum to 1 across generators.

    Attributes:
        w_complexity:  Weight on cyclomatic_quality within Φ_SIMPLE.
                       Entropy gets ``1 - w_complexity``.
        w_coupling:    Weight on coupling_quality within Φ_COMPOSABLE.
                       Instability gets ``1 - w_coupling``.
        w_taint:       Weight on taint_quality within Φ_SECURE.
                       Dangerous-API reachability gets ``1 - w_taint``.
    """

    w_complexity: float
    w_coupling: float
    w_taint: float


WEIGHT_PROFILES: dict[Priority, WeightProfile] = {
    Priority.BALANCED: WeightProfile(
        w_complexity=0.5, w_coupling=0.5, w_taint=0.5
    ),
    Priority.SIMPLE: WeightProfile(
        w_complexity=0.7, w_coupling=0.3, w_taint=0.3
    ),
    Priority.COMPOSABLE: WeightProfile(
        w_complexity=0.3, w_coupling=0.7, w_taint=0.3
    ),
    Priority.SECURE: WeightProfile(
        w_complexity=0.3, w_coupling=0.3, w_taint=0.7
    ),
}


@dataclass(frozen=True)
class ScoredDecision:
    """
    Result of applying one policy translator Φᵢ : ℝ → Ω.

    Attributes:
        score:          Quality score in [0.0, 1.0]; higher is better.
        achieved:       True when ``score >= threshold`` — i.e. the
                        generator gᵢ is satisfied for this program.
        interpretation: Per-metric human-readable strings keyed by
                        metric name (e.g. ``cfg.cyclomatic``).
    """

    score: float
    achieved: bool
    interpretation: dict[str, str] = field(default_factory=dict)
