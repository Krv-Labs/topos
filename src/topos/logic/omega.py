"""
Omega Module — The Subobject Classifier Ω
-----------------------------------------

This module *is* the subobject classifier Ω of the Topos
E = Set^(C x H^op).  Per the math spec §3, for every program P ∈ E and
every subprogram S ↪ P there exists a unique characteristic morphism

    χ_S : P -> Ω

mapping each structural component to an element of ℋ = H(G_qual), the
free Heyting algebra on three quality generators (SIMPLE, COMPOSABLE,
SECURE).

The codomain ℋ has two distinguished elements:

    ⊤ = IDEAL = ⋀_{i} g_i   (meet of all generators — the ideal program)
    ⊥ = SLOP                (unconstrained universe — no generator satisfied)

The classifier:

1. Builds every available Representation (AST + CFG + DependencyGraph +
   CPG) for the morphism.
2. Groups them by generator (each Representation declares its
   ``dimension`` ∈ {"simple", "composable", "secure"}).
3. Runs the matching policy translator Φᵢ on the collected metrics
   (``simple`` → Φ_SIMPLE, etc.).
4. Combines the three Boolean truth values via
   ``lattice.verdict_from_generators`` into the final ℋ element.

Priority shifts metric weights within each Φᵢ but does *not* change which
generators are pairwise incomparable — that is fixed by the math.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import (
    EvaluationLattice,
    EvaluationValue,
    verdict_from_generators,
)
from topos.logic.policies import (
    Priority,
    ScoredDecision,
    build_evaluation_lattice,
    score_coupling,
    score_secure,
    score_simple,
)

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism
    from topos.graphs.base import Representation


# ---------------------------------------------------------------------------
# Score dispatchers — one per Representation name
# ---------------------------------------------------------------------------
# Each dispatcher reads the namespaced raw-metric floats and returns a
# ScoredDecision via the matching policy translator Φᵢ.


def _score_cfg(raw: dict[str, float], priority: Priority) -> ScoredDecision | None:
    if "cfg.cyclomatic" not in raw:
        return None
    # Entropy is optional; if a parallel AST representation already
    # supplied ``ast.entropy`` in the same "simple" dimension it gets
    # mixed in by SubobjectClassifier (see classify_detailed's mixed
    # path) — so cfg dispatcher itself only consumes cyclomatic.
    return score_simple(
        cyclomatic=raw["cfg.cyclomatic"],
        entropy=None,
        priority=priority,
    )


def _score_ast(raw: dict[str, float], priority: Priority) -> ScoredDecision | None:
    # Legacy AST contribution to SIMPLE: feeds entropy only.  Cyclomatic
    # has moved to ``cfg.cyclomatic``.  Returning None when entropy is
    # absent keeps the classifier from double-scoring AST.
    if "ast.entropy" not in raw:
        return None
    return score_simple(
        cyclomatic=raw.get("ast.complexity", 0.0),
        entropy=raw["ast.entropy"],
        priority=priority,
    )


def _score_depgraph(
    raw: dict[str, float], priority: Priority
) -> ScoredDecision | None:
    if "depgraph.coupling" not in raw or "depgraph.instability" not in raw:
        return None
    return score_coupling(
        raw["depgraph.coupling"], raw["depgraph.instability"], priority
    )


def _score_cpg(raw: dict[str, float], priority: Priority) -> ScoredDecision | None:
    if "cpg.dangerous_calls" not in raw and "cpg.taint_flows" not in raw:
        return None
    return score_secure(
        dangerous_calls=raw.get("cpg.dangerous_calls", 0.0),
        taint_flows=raw.get("cpg.taint_flows", 0.0),
        priority=priority,
    )


_REPRESENTATION_SCORE_DISPATCHERS: dict[
    str,
    Callable[[dict[str, float], Priority], ScoredDecision | None],
] = {
    "cfg": _score_cfg,
    "ast": _score_ast,  # legacy: entropy contribution to SIMPLE
    "depgraph": _score_depgraph,
    "cpg": _score_cpg,
}

# Map each *dimension* name to the singleton generator value it produces
# when satisfied.  These three generators are pairwise incomparable in ℋ.
_DIMENSION_GENERATOR: dict[str, EvaluationValue] = {
    "simple": EvaluationValue.SIMPLE,
    "composable": EvaluationValue.COMPOSABLE,
    "secure": EvaluationValue.SECURE,
}


# ---------------------------------------------------------------------------
# Result dataclass
# ---------------------------------------------------------------------------


@dataclass
class ClassificationResult:
    """
    The result of applying χ_S : P → Ω to a program morphism.

    Attributes:
        is_parseable:    Whether the code parsed successfully.
        dimensions:      Per-generator lattice value: the singleton generator
                         (SIMPLE/COMPOSABLE/SECURE) when satisfied, SLOP
                         otherwise.
        scores:          Per-generator normalized quality score in [0.0, 1.0].
        lattice_element: Overall ℋ element — the join of the satisfied
                         generators, encoded via ``verdict_from_generators``.
        priority:        The Priority profile used during classification.
        raw_metrics:     All raw metric floats, namespaced by representation.
        interpretation:  Per-metric interpretation strings.
    """

    is_parseable: bool
    dimensions: dict[str, EvaluationValue] = field(default_factory=dict)
    scores: dict[str, float] = field(default_factory=dict)
    lattice_element: EvaluationValue = field(default=EvaluationValue.SLOP)
    priority: Priority = field(default=Priority.BALANCED)
    raw_metrics: dict[str, float] = field(default_factory=dict)
    interpretation: dict[str, str] = field(default_factory=dict)

    def summary(self) -> EvaluationValue:
        """The overall ℋ element."""
        return self.lattice_element

    def __str__(self) -> str:
        if not self.is_parseable:
            return "Classification: ⊥ SLOP (parse failure)"
        parts = [f"Classification: {self.lattice_element}"]
        for dim, val in self.dimensions.items():
            score_pct = f"{self.scores.get(dim, 0.0) * 100:.0f}%"
            parts.append(f"  {dim}: {val}  [{score_pct}]")
        for k, v in self.raw_metrics.items():
            parts.append(f"    {k}: {v:.3f}")
        return "\n".join(parts)


# ---------------------------------------------------------------------------
# Classifier
# ---------------------------------------------------------------------------


@dataclass
class SubobjectClassifier:
    """
    The Subobject Classifier Ω for the Topos of Programs.

    Implements the characteristic map χ : Program → Ω whose codomain is
    the free Heyting algebra ℋ = H(G_qual).  Each generator gᵢ is fed by
    the Representation theory says is the correct lens for that quality:

        SIMPLE      ← CFG cyclomatic complexity
        COMPOSABLE  ← Dependency-graph coupling/instability
        SECURE      ← Code Property Graph taint/danger probes

    Attributes:
        omega: The EvaluationLattice ℋ.
    """

    omega: EvaluationLattice = field(default_factory=build_evaluation_lattice)

    def classify(self, morphism: ProgramMorphism) -> EvaluationValue:
        """Return ``classify_detailed(...).summary()`` — the overall ℋ element."""
        return self.classify_detailed(morphism).summary()

    def classify_detailed(
        self,
        morphism: ProgramMorphism,
        representations: Sequence[Representation] | None = None,
        priority: Priority = Priority.BALANCED,
    ) -> ClassificationResult:
        """
        Compute χ_S : P → Ω in full detail.

        An ``ASTRepresentation`` is always built from the morphism (it
        carries ``ast.entropy`` into the SIMPLE generator).  Any additional
        *representations* (CFG, DependencyGraph, PDG, CPG) are grouped by
        their ``dimension`` and scored independently.

        Parse failures collapse to ⊥ = SLOP.
        """
        if morphism.ast is None or not morphism.is_valid:
            return ClassificationResult(is_parseable=False, priority=priority)

        # Always include an ASTRepresentation (entropy → SIMPLE).
        from topos.graphs.ast.object import ASTRepresentation

        ast_rep = ASTRepresentation(
            program_object=morphism.ast,
            source=morphism.source,
        )
        all_reps: list[Representation] = [ast_rep]
        if representations:
            all_reps.extend(representations)

        by_dimension: dict[str, list[Representation]] = defaultdict(list)
        for rep in all_reps:
            by_dimension[rep.dimension].append(rep)

        raw_metrics: dict[str, float] = {}
        interpretation: dict[str, str] = {}
        dimensions: dict[str, EvaluationValue] = {}
        scores: dict[str, float] = {}

        for dim, reps in by_dimension.items():
            dim_raw: dict[str, float] = {}
            for rep in reps:
                dim_raw.update(rep.metrics())
            raw_metrics.update(dim_raw)

            rep_names = {rep.name for rep in reps}

            if len(rep_names) == 1:
                rep_name = reps[0].name
                scorer = _REPRESENTATION_SCORE_DISPATCHERS.get(rep_name)
                if not scorer:
                    continue
                decision = scorer(dim_raw, priority)
                if decision is None:
                    continue
            else:
                # Mixed representations within one dimension: score each
                # independently and meet the truth values (= min score,
                # AND on achieved).  This is how CFG (cyclomatic) and
                # AST (entropy) jointly feed the SIMPLE generator.
                mixed_scores: list[float] = []
                mixed_interp: dict[str, str] = {}
                mixed_achieved = True
                any_scored = False

                for rep in reps:
                    scorer = _REPRESENTATION_SCORE_DISPATCHERS.get(rep.name)
                    if not scorer:
                        continue
                    rep_decision = scorer(rep.metrics(), priority)
                    if rep_decision is None:
                        continue
                    any_scored = True
                    mixed_scores.append(rep_decision.score)
                    mixed_interp.update(rep_decision.interpretation)
                    mixed_achieved = mixed_achieved and rep_decision.achieved

                if not any_scored:
                    continue
                decision = ScoredDecision(
                    score=min(mixed_scores),
                    achieved=mixed_achieved,
                    interpretation=mixed_interp,
                )

            scores[dim] = decision.score
            interpretation.update(decision.interpretation)
            generator = _DIMENSION_GENERATOR.get(dim, EvaluationValue.SLOP)
            dimensions[dim] = generator if decision.achieved else EvaluationValue.SLOP

        # Assemble the overall ℋ element from the achieved generators.
        lattice_element = verdict_from_generators(
            simple=dimensions.get("simple") == EvaluationValue.SIMPLE,
            composable=dimensions.get("composable") == EvaluationValue.COMPOSABLE,
            secure=dimensions.get("secure") == EvaluationValue.SECURE,
        )

        return ClassificationResult(
            is_parseable=True,
            dimensions=dimensions,
            scores=scores,
            lattice_element=lattice_element,
            priority=priority,
            raw_metrics=raw_metrics,
            interpretation=interpretation,
        )

    def combine(self, *values: EvaluationValue) -> EvaluationValue:
        """Combine multiple ℋ values via meet (∧)."""
        return self.omega.combine(*values)

    def combine_dimensions(
        self,
        results: Iterable[ClassificationResult],
        threshold: float = 0.6,
    ) -> dict[str, EvaluationValue]:
        """
        Pointwise multi-file meet ⋀_f χ_S(f).

        A generator is satisfied across the codebase iff it is satisfied
        for every file (minimum score across files ≥ threshold).  Parse
        failures inject a zero score on the SIMPLE generator (since the
        program failed even to compile, no other generator is reachable).
        """
        min_scores: dict[str, float] = {}
        for result in results:
            if not result.is_parseable:
                min_scores["simple"] = min(min_scores.get("simple", 1.0), 0.0)
            for dim, score in result.scores.items():
                if dim not in min_scores or score < min_scores[dim]:
                    min_scores[dim] = score

        combined: dict[str, EvaluationValue] = {}
        for dim, score in min_scores.items():
            generator = _DIMENSION_GENERATOR.get(dim, EvaluationValue.SLOP)
            combined[dim] = generator if score >= threshold else EvaluationValue.SLOP
        return combined
