"""
Policy translators and auxiliary decision helpers.

**Quality generators (Ω):** ``score_simple``, ``score_coupling``, and
``score_secure`` each map probe metrics to a :class:`ScoredDecision`.
``achieved`` is the AND of per-metric **raw** gates from
:mod:`topos.evaluation.policies.calibration`; the characteristic morphism
combines the three booleans into an element of Ω = H(G_qual).

**Outside Ω:** ``are_clones`` (pairwise AST distance) and
``score_declaration_coverage`` (structural test coverage) apply the same
"functors measure, policies decide" split without participating in the
three-generator lattice.
"""

from topos.evaluation.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
    WeightProfile,
)
from topos.evaluation.policies.clones import are_clones
from topos.evaluation.policies.composable import score_coupling
from topos.evaluation.policies.coverage import (
    CoverageDecision,
    score_declaration_coverage,
)
from topos.evaluation.policies.secure import score_secure
from topos.evaluation.policies.simple import (
    build_omega,
    describe_entropy_ratio,
    score_simple,
)
from topos.evaluation.policies.calibration import (
    CLONE,
    COMPOSABLE,
    COVERAGE,
    SCORE_FLOORS,
    SECURE,
    SIMPLE,
)

__all__ = [
    "Priority",
    "WeightProfile",
    "WEIGHT_PROFILES",
    "ScoredDecision",
    "CoverageDecision",
    "score_declaration_coverage",
    "are_clones",
    "score_simple",
    "score_coupling",
    "score_secure",
    "describe_entropy_ratio",
    "build_omega",
    "SIMPLE",
    "COMPOSABLE",
    "SECURE",
    "COVERAGE",
    "CLONE",
    "SCORE_FLOORS",
]
