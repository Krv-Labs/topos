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
    EvaluationLattice (diamond), and the characteristic map evaluates code
    quality along two independent *dimensions*.

Diamond Lattice Evaluation Model:
    Two dimensions measure orthogonal program qualities:
        - "structural": AST metrics (complexity, entropy) → SELF_CONTAINED target
        - "coupling":   Dep-graph metrics (coupling, instability) → COMPOSABLE target

    Each dimension produces a normalized quality score in [0, 1].  A score
    ≥ threshold means the lattice target for that dimension is achieved.

    The overall lattice element is determined by which targets are met:
        Both achieved   → SOUND          (⊤)
        Structural only → SELF_CONTAINED
        Coupling only   → COMPOSABLE
        Neither         → BROKEN         (⊥)

    COMPOSABLE requires a DependencyGraph representation; it is unreachable
    from AST metrics alone.

Priority:
    An optional Priority parameter shifts metric weights within each dimension,
    letting agents express which quality axis matters most for the current task.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.policies import (
    Priority,
    ScoredDecision,
    build_evaluation_lattice,
    score_coupling,
    score_structural,
)

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism
    from topos.graphs.base import Representation


# ---------------------------------------------------------------------------
# Score dispatchers
# ---------------------------------------------------------------------------
# Maps representation *name* to a function that converts raw metric floats
# into a ScoredDecision for that representation's dimension.


def _score_ast(raw: dict[str, float], priority: Priority) -> ScoredDecision | None:
    if "ast.complexity" not in raw or "ast.entropy" not in raw:
        return None
    return score_structural(raw["ast.complexity"], raw["ast.entropy"], priority)


def _score_depgraph(raw: dict[str, float], priority: Priority) -> ScoredDecision | None:
    if "depgraph.coupling" not in raw or "depgraph.instability" not in raw:
        return None
    return score_coupling(
        raw["depgraph.coupling"], raw["depgraph.instability"], priority
    )


_REPRESENTATION_SCORE_DISPATCHERS: dict[
    str,
    Callable[[dict[str, float], Priority], ScoredDecision | None],
] = {
    "ast": _score_ast,
    "depgraph": _score_depgraph,
}

# Map each representation name to its lattice target when achieved.
_DIMENSION_TARGET: dict[str, EvaluationValue] = {
    "structural": EvaluationValue.SELF_CONTAINED,
    "coupling": EvaluationValue.COMPOSABLE,
}


# ---------------------------------------------------------------------------
# Result dataclass
# ---------------------------------------------------------------------------


@dataclass
class ClassificationResult:
    """
    The result of classifying a program morphism.

    Attributes:
        is_parseable:    Whether the code parsed successfully.
        dimensions:      Per-axis lattice target: SELF_CONTAINED or COMPOSABLE
                         when achieved, BROKEN otherwise.
        scores:          Per-axis normalized quality score in [0.0, 1.0].
        lattice_element: Overall lattice element combining all dimensions.
        priority:        The Priority profile used during classification.
        raw_metrics:     All raw metric floats, namespaced by representation.
        interpretation:  Per-metric interpretation strings.
    """

    is_parseable: bool
    dimensions: dict[str, EvaluationValue] = field(default_factory=dict)
    scores: dict[str, float] = field(default_factory=dict)
    lattice_element: EvaluationValue = field(default=EvaluationValue.BROKEN)
    priority: Priority = field(default=Priority.BALANCED)
    raw_metrics: dict[str, float] = field(default_factory=dict)
    interpretation: dict[str, str] = field(default_factory=dict)

    # ------------------------------------------------------------------
    # Convenience
    # ------------------------------------------------------------------

    def summary(self) -> EvaluationValue:
        """
        The overall lattice element.

        Use this when a single value is needed (e.g. multi-file rollup,
        backward-compat API).  For per-dimension detail use ``dimensions``
        and ``scores``.
        """
        return self.lattice_element

    def __str__(self) -> str:
        if not self.is_parseable:
            return "Classification: ⊥ BROKEN (parse failure)"
        parts = []
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
    The Subobject Classifier (Ω) for the category of Programs.

    This is the core evaluation engine of topos. It maps program
    morphisms to evaluation values in the Heyting Algebra, implementing
    the characteristic map χ: Program → Ω.

    Each representation is scored independently and mapped to its lattice
    target (SELF_CONTAINED for structural, COMPOSABLE for coupling).  The
    overall lattice element is determined by which targets are achieved.

    Attributes:
        omega: The EvaluationLattice (our Ω).
    """

    omega: EvaluationLattice = field(default_factory=build_evaluation_lattice)

    def classify(self, morphism: ProgramMorphism) -> EvaluationValue:
        """
        Map a ProgramMorphism to an EvaluationValue.

        Returns ``ClassificationResult.summary()`` — the overall lattice
        element.  For per-dimension detail use ``classify_detailed``.
        """
        return self.classify_detailed(morphism).summary()

    def classify_detailed(
        self,
        morphism: ProgramMorphism,
        representations: Sequence[Representation] | None = None,
        priority: Priority = Priority.BALANCED,
    ) -> ClassificationResult:
        """
        Perform detailed per-dimension classification.

        An ``ASTRepresentation`` is always built from the morphism and
        contributes to the ``"structural"`` dimension.  Any additional
        *representations* are grouped by their ``dimension`` property and
        scored independently.

        Args:
            morphism:        The program to classify.
            representations: Optional extra representations (e.g. a
                             ``DependencyGraph`` for the ``"coupling"`` dimension).
            priority:        Weight profile shifting which metrics are
                             emphasised within each dimension.

        Returns:
            A ``ClassificationResult`` with the overall lattice element,
            per-dimension scores, and raw metrics.  When the code fails to
            parse, ``is_parseable`` is ``False`` and ``lattice_element`` is
            ``BROKEN``.
        """
        if morphism.ast is None or not morphism.is_valid:
            return ClassificationResult(is_parseable=False, priority=priority)

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
        scores: dict[str, float] = {}

        for dim, reps in by_dimension.items():
            # Collect all raw metrics for this dimension.
            dim_raw: dict[str, float] = {}
            for rep in reps:
                dim_raw.update(rep.metrics())
            raw_metrics.update(dim_raw)

            rep_names = {rep.name for rep in reps}

            if len(rep_names) == 1:
                # Preserve the existing behavior when all representations in the
                # dimension share the same type.
                rep_name = reps[0].name
                scorer = _REPRESENTATION_SCORE_DISPATCHERS.get(rep_name)
                if not scorer:
                    continue

                decision = scorer(dim_raw, priority)
                if decision is None:
                    continue
            else:
                # Mixed representation types within one dimension must be scored
                # independently so dispatcher selection does not depend on reps[0].
                mixed_scores: list[float] = []
                mixed_interpretation: dict[str, str] = {}
                mixed_achieved = True

                for rep in reps:
                    scorer = _REPRESENTATION_SCORE_DISPATCHERS.get(rep.name)
                    if not scorer:
                        continue

                    rep_decision = scorer(rep.metrics(), priority)
                    if rep_decision is None:
                        continue

                    mixed_scores.append(rep_decision.score)
                    mixed_interpretation.update(rep_decision.interpretation)
                    mixed_achieved = mixed_achieved and rep_decision.achieved

                if not mixed_scores:
                    continue

                decision = ScoredDecision(
                    score=min(mixed_scores),
                    achieved=mixed_achieved,
                    interpretation=mixed_interpretation,
                )
            scores[dim] = decision.score
            interpretation.update(decision.interpretation)

            target = _DIMENSION_TARGET.get(dim, EvaluationValue.BROKEN)
            dimensions[dim] = target if decision.achieved else EvaluationValue.BROKEN

        # Assemble the overall lattice element from achieved targets.
        structural_achieved = (
            dimensions.get("structural") == EvaluationValue.SELF_CONTAINED
        )
        coupling_achieved = dimensions.get("coupling") == EvaluationValue.COMPOSABLE

        if structural_achieved and coupling_achieved:
            lattice_element = EvaluationValue.SOUND
        elif structural_achieved:
            lattice_element = EvaluationValue.SELF_CONTAINED
        elif coupling_achieved:
            lattice_element = EvaluationValue.COMPOSABLE
        else:
            lattice_element = EvaluationValue.BROKEN

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
        threshold: float = 0.6,
    ) -> dict[str, EvaluationValue]:
        """
        Aggregate per-dimension verdicts across multiple files.

        Uses minimum score across files per dimension, then re-applies the
        threshold to determine the lattice target.  Files without a given
        dimension are skipped for that axis.

        Args:
            results:   Per-file ClassificationResult instances.
            threshold: Score threshold for achieving a lattice target.

        Returns:
            A dict mapping dimension names to their combined lattice target.
        """
        min_scores: dict[str, float] = {}
        for result in results:
            for dim, score in result.scores.items():
                if dim not in min_scores or score < min_scores[dim]:
                    min_scores[dim] = score

        combined: dict[str, EvaluationValue] = {}
        for dim, score in min_scores.items():
            target = _DIMENSION_TARGET.get(dim, EvaluationValue.BROKEN)
            combined[dim] = target if score >= threshold else EvaluationValue.BROKEN
        return combined
