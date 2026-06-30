"""
Φ_PROCESS — policy translators for GitNexus execution-flow metrics.
------------------------------------------------------------------

GitNexus execution flows give an *interprocedural* lens on each pillar. Rather
than introduce a fourth generator, these translators emit per-pillar
:class:`~topos.evaluation.policies.base.ScoredDecision` contributions that the
characteristic morphism **merges** into the existing SIMPLE / COMPOSABLE /
SECURE decisions (``achieved`` AND-ed, ``score`` min-ed):

    score_process_simple      → SIMPLE      (flow length / participation)
    score_process_composable  → COMPOSABLE  (community span / crossings)
    score_process_secure      → SECURE      (dangerous-step reachability)

Every metric is optional; a translator with no metrics returns a vacuous
"achieved" decision, so behavior is unchanged when no ``.gitnexus`` index is
present. Thresholds live in
:class:`~topos.evaluation.policies.calibration.ProcessPolicyThresholds`
(provisional — see ``docs/process-flow-spike.md``).
"""

from __future__ import annotations

from math import exp

from topos.evaluation.policies.base import ScoredDecision
from topos.evaluation.policies.calibration import PROCESS


def score_process_simple(
    max_flow_length: float | None = None,
    flow_participation: float | None = None,
) -> ScoredDecision:
    """Φ_PROCESS·SIMPLE — interprocedural flow complexity."""
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    if max_flow_length is not None:
        qualities.append(1.0 - min(max_flow_length / PROCESS.max_flow_length_cap, 1.0))
        if max_flow_length > PROCESS.max_flow_length:
            achieved = False
        interp["process.max_flow_length"] = _gate_interpretation(
            "longest execution flow",
            max_flow_length,
            PROCESS.max_flow_length,
            unit="steps",
        )

    if flow_participation is not None:
        qualities.append(
            1.0 - min(flow_participation / PROCESS.max_flow_participation_cap, 1.0)
        )
        if flow_participation > PROCESS.max_flow_participation:
            achieved = False
        interp["process.flow_participation"] = _gate_interpretation(
            "execution flows through this file",
            flow_participation,
            PROCESS.max_flow_participation,
            unit="flows",
        )

    return _decision(qualities, achieved, interp)


def score_process_composable(
    max_community_span: float | None = None,
    cross_community_flows: float | None = None,
) -> ScoredDecision:
    """Φ_PROCESS·COMPOSABLE — flow-level coupling across communities."""
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    if max_community_span is not None:
        qualities.append(
            1.0 - min(max_community_span / PROCESS.max_community_span_cap, 1.0)
        )
        if max_community_span > PROCESS.max_community_span:
            achieved = False
        interp["process.max_community_span"] = _gate_interpretation(
            "communities spanned by a single flow",
            max_community_span,
            PROCESS.max_community_span,
            unit="communities",
        )

    if cross_community_flows is not None:
        qualities.append(
            1.0
            - min(cross_community_flows / PROCESS.max_cross_community_flows_cap, 1.0)
        )
        if cross_community_flows > PROCESS.max_cross_community_flows:
            achieved = False
        interp["process.cross_community_flows"] = _gate_interpretation(
            "cross-community flows through this file",
            cross_community_flows,
            PROCESS.max_cross_community_flows,
            unit="flows",
        )

    return _decision(qualities, achieved, interp)


def score_process_secure(dangerous_flows: float | None = None) -> ScoredDecision:
    """Φ_PROCESS·SECURE — dangerous-API reachability on an execution flow."""
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    if dangerous_flows is not None:
        qualities.append(exp(-max(dangerous_flows, 0.0) / PROCESS.dangerous_flow_scale))
        if dangerous_flows > PROCESS.max_dangerous_flows:
            achieved = False
        if dangerous_flows <= PROCESS.max_dangerous_flows:
            interp["process.dangerous_flows"] = (
                f"no dangerous-API steps on any execution flow "
                f"({dangerous_flows:.0f} <= {PROCESS.max_dangerous_flows:.0f})"
            )
        else:
            interp["process.dangerous_flows"] = (
                f"{int(dangerous_flows)} execution flow(s) reach a dangerous-API "
                f"step (> {PROCESS.max_dangerous_flows:.0f})"
            )

    return _decision(qualities, achieved, interp)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _decision(
    qualities: list[float], achieved: bool, interp: dict[str, str]
) -> ScoredDecision:
    if not qualities:
        return ScoredDecision(score=1.0, achieved=True, interpretation={})
    return ScoredDecision(
        score=min(qualities), achieved=achieved, interpretation=interp
    )


def _gate_interpretation(label: str, raw: float, gate: float, *, unit: str) -> str:
    if raw <= gate:
        return f"{label} ({raw:.0f} {unit}) within threshold (<= {gate:.0f})"
    return f"{label} ({raw:.0f} {unit}) exceeds threshold (> {gate:.0f})"
