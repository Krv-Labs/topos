"""
Omega Module (The Subobject Classifier)
---------------------------------------
The 'Subobject Classifier' (Ω) is the soul of a Topos. It provides a
characteristic map that assigns an evaluation value from our Heyting Algebra
to any 'subobject' (piece of code).

Mathematical Inspiration:
    For every subobject 's' of 'X', there exists a unique morphism
    χ: X → Ω. We use this to classify code by its structural quality.

    In Set (the category of sets), Ω = {0, 1} and the characteristic
    function is simply membership. In our Topos of Programs, Ω is the
    EvaluationLattice, and the characteristic map evaluates code quality
    along multiple *dimensions*.

Per-Dimension Evaluation Model:
    Different representations measure orthogonal program qualities.  AST
    metrics capture *internal structural quality* (branching, entropy).
    Dependency-graph metrics capture *coupling quality* (how well-positioned
    the module is in the overall architecture).  These axes are independent
    and should not be collapsed into a single verdict via meet.

    ``classify_detailed`` groups representations by their ``dimension``
    property, aggregates metrics within each group via the lattice's
    non-total partial order, and returns a ``ClassificationResult`` whose
    ``dimensions`` dict holds one ``EvaluationValue`` per axis.  Dimensions
    are never merged automatically.

    To get a single scalar for tooling that needs one number, call
    ``ClassificationResult.summary()`` — it returns the worst value across
    dimensions.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.policies import (
    MetricDecision,
    build_evaluation_lattice,
    classify_complexity,
    classify_entropy,
)

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism
    from topos.graphs.base import Representation


# ---------------------------------------------------------------------------
# Verdict dispatchers
# ---------------------------------------------------------------------------
# Maps representation *name* to a function that converts raw metric floats
# into {metric_name: EvaluationValue} dicts.  Keyed by rep.name (not
# rep.dimension) so different representations on the same dimension can each
# have their own verdict logic.

def _ast_verdicts(raw: dict[str, float]) -> dict[str, MetricDecision]:
    decisions: dict[str, MetricDecision] = {}
    if "ast.complexity" in raw:
        decisions["ast.complexity"] = classify_complexity(raw["ast.complexity"])
    if "ast.entropy" in raw:
        decisions["ast.entropy"] = classify_entropy(raw["ast.entropy"])
    return decisions


def _depgraph_verdicts(raw: dict[str, float]) -> dict[str, MetricDecision]:
    from topos.logic.policies import classify_coupling, classify_instability

    decisions: dict[str, MetricDecision] = {}
    if "depgraph.coupling" in raw:
        decisions["depgraph.coupling"] = classify_coupling(raw["depgraph.coupling"])
    if "depgraph.instability" in raw:
        decisions["depgraph.instability"] = classify_instability(
            raw["depgraph.instability"]
        )
    return decisions


_REPRESENTATION_VERDICT_DISPATCHERS: dict[
    str,
    Callable[[dict[str, float]], dict[str, MetricDecision]],
] = {
    "ast": _ast_verdicts,
    "depgraph": _depgraph_verdicts,
}


# ---------------------------------------------------------------------------
# Result dataclass
# ---------------------------------------------------------------------------

@dataclass
class ClassificationResult:
    """
    The result of classifying a program morphism.

    Attributes:
        is_parseable: Whether the code parsed successfully. When False,
            ``dimensions`` is empty and the code cannot be evaluated.
        dimensions: Per-quality-axis evaluation values.  Keys are dimension
            names (e.g. ``"structural"``, ``"coupling"``); values are
            ``EvaluationValue`` instances representing the worst metric
            verdict within that dimension.
        raw_metrics: All raw metric floats, namespaced by representation
            (e.g. ``{"ast.complexity": 12.0, "depgraph.coupling": 7.0}``).
        interpretation: Per-metric interpretation strings from the bins.
    """

    is_parseable: bool
    dimensions: dict[str, EvaluationValue] = field(default_factory=dict)
    raw_metrics: dict[str, float] = field(default_factory=dict)
    interpretation: dict[str, str] = field(default_factory=dict)

    # ------------------------------------------------------------------
    # Convenience
    # ------------------------------------------------------------------

    def summary(self) -> EvaluationValue:
        """
        Worst (lowest) value across all dimensions.

        Use this when a single scalar is needed (e.g. multi-file rollup,
        backward-compat API).  Note that this collapses orthogonal axes and
        loses information — prefer ``dimensions`` for display.
        """
        if not self.dimensions:
            return EvaluationValue.BROKEN
        return min(self.dimensions.values(), key=lambda v: v.value)

    def __str__(self) -> str:
        if not self.is_parseable:
            return "Classification: ⊥ BROKEN (parse failure)"
        parts = []
        for dim, val in self.dimensions.items():
            parts.append(f"  {dim}: {val}")
        for k, v in self.raw_metrics.items():
            parts.append(f"    {k}: {v:.3f}")
        return "\n".join(parts)


# ---------------------------------------------------------------------------
# Classifier
# ---------------------------------------------------------------------------

@dataclass
class SubobjectClassifier:
    """
    The Subobject Classifier (Ω) for the category of Programs.

    This is the core evaluation engine of topos. It maps program
    morphisms to evaluation values in the Heyting Algebra, implementing
    the characteristic map χ: Program → Ω.

    The classifier aggregates metrics from representations grouped by their
    ``dimension`` property.  Representations in the same dimension are
    combined via lattice meet; representations in different dimensions
    produce independent verdicts that are never merged automatically.

    Attributes:
        omega: The EvaluationLattice (our Ω).
    """

    omega: EvaluationLattice = field(default_factory=build_evaluation_lattice)

    def classify(self, morphism: ProgramMorphism) -> EvaluationValue:
        """
        Map a ProgramMorphism to an EvaluationValue.

        Returns ``ClassificationResult.summary()`` — the worst value across
        all dimensions.  For per-dimension detail use ``classify_detailed``.
        """
        return self.classify_detailed(morphism).summary()

    def classify_detailed(
        self,
        morphism: ProgramMorphism,
        representations: Sequence[Representation] | None = None,
    ) -> ClassificationResult:
        """
        Perform detailed per-dimension classification.

        An ``ASTRepresentation`` is always built from the morphism and
        contributes to the ``"structural"`` dimension.  Any additional
        *representations* are grouped by their ``dimension`` property;
        within each group their metric verdicts are combined via lattice meet.

        Args:
            morphism: The program to classify.
            representations: Optional extra representations (e.g. a
                ``DependencyGraph`` for the ``"coupling"`` dimension).

        Returns:
            A ``ClassificationResult`` with per-dimension verdicts and raw
            metrics.  When the code fails to parse, ``is_parseable`` is
            ``False`` and ``dimensions`` is empty.
        """
        if morphism.ast is None or not morphism.is_valid:
            return ClassificationResult(is_parseable=False)

        # Always include an ASTRepresentation for the structural dimension.
        from topos.graphs.ast.object import ASTRepresentation

        ast_rep = ASTRepresentation(
            program_object=morphism.ast,
            source=morphism.source,
        )
        all_reps: list[Representation] = [ast_rep]
        if representations:
            all_reps.extend(representations)

        # Group representations by dimension.
        by_dimension: dict[str, list[Representation]] = defaultdict(list)
        for rep in all_reps:
            by_dimension[rep.dimension].append(rep)

        raw_metrics: dict[str, float] = {}
        interpretation: dict[str, str] = {}
        dimensions: dict[str, EvaluationValue] = {}

        for dim, reps in by_dimension.items():
            dim_verdicts: dict[str, EvaluationValue] = {}

            for rep in reps:
                rep_raw = rep.metrics()
                raw_metrics.update(rep_raw)

                dispatcher = _REPRESENTATION_VERDICT_DISPATCHERS.get(rep.name)
                if dispatcher:
                    rep_decisions = dispatcher(rep_raw)
                    for metric_name, decision in rep_decisions.items():
                        dim_verdicts[metric_name] = decision.evaluation
                        interpretation[metric_name] = decision.interpretation

            dimensions[dim] = self.omega.aggregate(dim_verdicts)

        return ClassificationResult(
            is_parseable=True,
            dimensions=dimensions,
            raw_metrics=raw_metrics,
            interpretation=interpretation,
        )

    def combine(self, *values: EvaluationValue) -> EvaluationValue:
        """
        Combine multiple evaluation values using meet (∧).

        When evaluating a codebase with multiple files (summary mode),
        the overall evaluation is the meet of all individual evaluations.
        Use ``combine_dimensions`` for dimension-aware multi-file rollup.
        """
        return self.omega.combine(*values)

    def combine_dimensions(
        self,
        results: Iterable[ClassificationResult],
    ) -> dict[str, EvaluationValue]:
        """
        Aggregate per-dimension verdicts across multiple files.

        For each dimension, computes the meet of all file-level verdicts.
        Files that don't have a given dimension are skipped for that axis.

        Returns:
            A dict mapping dimension names to their combined verdict.
        """
        accum: dict[str, EvaluationValue] = {}
        for result in results:
            for dim, val in result.dimensions.items():
                if dim in accum:
                    accum[dim] = self.omega.meet(accum[dim], val)
                else:
                    accum[dim] = val
        return accum
