"""
Allowlist overlay — the *adjusted* SECURE verdict (anti-gaming design).

The core classification pipeline is canonical and untouched: it always
produces the raw verdict from the full ``DANGEROUS_APIS`` registry.  This
module computes an **adjusted** view *on top of* that result by re-counting
dangerous calls / taint flows with the allowlisted patterns removed.

Both verdicts are always surfaced together, every suppression is disclosed
with its mandatory reason, and any active suppression caps the attainable
grade below Gold/IDEAL.  An agent therefore cannot silently hide a finding
to inflate the score — only acknowledge it, visibly, and never to the top.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.config import AllowEntry, ToposConfig
from topos.core.omega import EvaluationValue, verdict_from_generators
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.functors.probes.cpg.danger import _matches_registry, dangerous_api_reachable
from topos.functors.probes.cpg.taint import taint_flow_paths
from topos.graphs.cpg.object import CodePropertyGraph
from topos.mcp.schemas import SecurityFinding


@dataclass(frozen=True)
class AdjustedVerdict:
    """Raw vs. allowlist-adjusted SECURE verdict for one file."""

    raw_secure_pass: bool
    adjusted_secure_pass: bool
    raw_element: EvaluationValue
    adjusted_element: EvaluationValue  # after the grade cap
    active_findings: list[SecurityFinding] = field(default_factory=list)
    acknowledged: list[tuple[SecurityFinding, AllowEntry]] = field(default_factory=list)
    grade_capped: bool = False  # True iff IDEAL was demoted due to suppression

    @property
    def suppressions_active(self) -> bool:
        return bool(self.acknowledged)

    @property
    def verdict_changed(self) -> bool:
        return self.raw_secure_pass != self.adjusted_secure_pass


def _entry_for_callee(
    callee: str | None, entries: list[AllowEntry]
) -> AllowEntry | None:
    """First allow entry whose pattern matches *callee* (suffix-aware)."""
    if not callee:
        return None
    for entry in entries:
        if _matches_registry(callee, {entry.pattern}):
            return entry
    return None


def apply_allowlist(
    result: ClassificationResult,
    findings: list[SecurityFinding],
    config: ToposConfig,
    *,
    file_path: str | None,
    cpg: CodePropertyGraph | None,
) -> AdjustedVerdict:
    """Overlay *config*'s allowlist onto a canonical classification result.

    *findings* are the raw findings (full registry).  *cpg* is used to
    recompute exact adjusted counts so the 20-finding display cap cannot
    corrupt the verdict.
    """
    raw_secure_pass = result.dimensions.get("secure") == EvaluationValue.SECURE
    raw_element = result.summary()

    entries = config.entries_for(file_path)

    # Partition raw findings into acknowledged vs. still-active.
    active: list[SecurityFinding] = []
    acknowledged: list[tuple[SecurityFinding, AllowEntry]] = []
    for finding in findings:
        entry = _entry_for_callee(finding.callee, entries)
        if entry is not None:
            acknowledged.append((finding, entry))
        else:
            active.append(finding)

    # Recompute the adjusted SECURE gate from exact counts (gate == 0).
    allow_patterns = {entry.pattern for entry in entries}
    if allow_patterns and cpg is not None:
        dangerous = dangerous_api_reachable(cpg, allow_patterns)
        taint = taint_flow_paths(cpg, allow_patterns)
        adjusted_secure_pass = dangerous == 0 and taint == 0
    else:
        adjusted_secure_pass = raw_secure_pass

    adjusted_element = _recompute_element(result, adjusted_secure_pass)

    # Grade cap: acknowledged risk can never buy the top medal.
    grade_capped = False
    if acknowledged and adjusted_element == EvaluationValue.IDEAL:
        adjusted_element = verdict_from_generators(
            simple=True, composable=True, secure=False
        )
        grade_capped = True

    return AdjustedVerdict(
        raw_secure_pass=raw_secure_pass,
        adjusted_secure_pass=adjusted_secure_pass,
        raw_element=raw_element,
        adjusted_element=adjusted_element,
        active_findings=active,
        acknowledged=acknowledged,
        grade_capped=grade_capped,
    )


def _recompute_element(
    result: ClassificationResult, secure_pass: bool
) -> EvaluationValue:
    """Rebuild the Ω element from dimensions, overriding the SECURE bit."""
    return verdict_from_generators(
        simple=result.dimensions.get("simple") == EvaluationValue.SIMPLE,
        composable=result.dimensions.get("composable") == EvaluationValue.COMPOSABLE,
        secure=secure_pass,
    )
