"""
Omega Module (The Subobject Classifier)
---------------------------------------
The 'Subobject Classifier' (Ω) is the soul of a Topos. It provides a
characteristic map that assigns an evaluation value from our Heyting Algebra
to any 'subobject' (piece of code).

Mathematical Inspiration:
    For every subobject 's' of 'X', there exists a unique morphism
    χ: X → Ω. We use this to classify 'commodity code' by its
    adherence to structural quality.

    In Set (the category of sets), Ω = {0, 1} and the characteristic
    function is simply membership. In our Topos of Programs, Ω is the
    EvaluationLattice, and the characteristic map evaluates code quality
    along multiple dimensions.

    The classifier combines metrics from *all* attached representations
    (AST, dependency graph, ...) into a single evaluation value via
    lattice aggregation.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.policies import (
    build_evaluation_lattice,
    classify_complexity,
    classify_entropy,
    normalize_complexity,
)
from topos.metrics.ast.complexity import calculate_cyclomatic_complexity
from topos.metrics.ast.entropy import calculate_kolmogorov_proxy

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism
    from topos.representations.base import Representation


# Maps representation names to functions that turn raw metrics
# into ``{metric_name: EvaluationValue}`` dicts.
def _depgraph_verdicts(raw: dict[str, float]) -> dict[str, EvaluationValue]:
    from topos.logic.dep_policies import classify_coupling, classify_instability

    verdicts: dict[str, EvaluationValue] = {}
    if "depgraph.coupling" in raw:
        verdicts["depgraph.coupling"] = classify_coupling(
            raw["depgraph.coupling"]
        ).evaluation
    if "depgraph.instability" in raw:
        verdicts["depgraph.instability"] = classify_instability(
            raw["depgraph.instability"]
        ).evaluation
    return verdicts


_REPRESENTATION_VERDICT_DISPATCHERS: dict[
    str,
    type[object] | None,
] = {
    "depgraph": None,  # sentinel; dispatch handled inline
}


@dataclass
class ClassificationResult:
    """
    The result of classifying a program morphism.

    Contains the final evaluation along with the individual
    metric scores that contributed to the classification.

    Attributes:
        evaluation: The final classification in the lattice.
        complexity_score: Normalized cyclomatic complexity (0-1).
        entropy_score: Normalized Kolmogorov proxy (0-1).
        is_valid: Whether the code parses successfully.
        metrics: Raw metric values for inspection.
        representation_metrics: Per-representation raw metric dicts.
    """

    evaluation: EvaluationValue
    complexity_score: float
    entropy_score: float
    is_valid: bool
    metrics: dict[str, object] = field(default_factory=dict)
    representation_metrics: dict[str, dict[str, float]] = field(default_factory=dict)

    def __str__(self) -> str:
        parts = [
            f"Classification: {self.evaluation}",
            f"  Complexity: {self.complexity_score:.2f}",
            f"  Entropy: {self.entropy_score:.2f}",
            f"  Valid Syntax: {self.is_valid}",
        ]
        for rep_name, rep_metrics in self.representation_metrics.items():
            parts.append(f"  [{rep_name}]")
            for k, v in rep_metrics.items():
                parts.append(f"    {k}: {v:.3f}")
        return "\n".join(parts)


@dataclass
class SubobjectClassifier:
    """
    The Subobject Classifier (Ω) for the category of Programs.

    This is the core evaluation engine of topos. It maps program
    morphisms to evaluation values in the Heyting Algebra, implementing
    the characteristic map χ: Program → Ω.

    The classifier combines metric verdicts from the AST representation
    and any additional attached representations to determine where a
    piece of code falls in the evaluation lattice.

    Attributes:
        omega: The EvaluationLattice (our Ω).
    """

    omega: EvaluationLattice = field(default_factory=build_evaluation_lattice)

    def classify(self, morphism: ProgramMorphism) -> EvaluationValue:
        """
        Map a ProgramMorphism to an EvaluationValue in the lattice.

        This is the core 'Evaluation' function of the library -- the
        implementation of the characteristic map χ: X → Ω.

        Args:
            morphism: The program to classify.

        Returns:
            An EvaluationValue representing the code's position in the lattice.
        """
        result = self.classify_detailed(morphism)
        return result.evaluation

    def classify_detailed(
        self,
        morphism: ProgramMorphism,
        representations: Sequence[Representation] | None = None,
    ) -> ClassificationResult:
        """
        Perform detailed classification with full metrics.

        The AST-based complexity/entropy evaluation always runs.  When
        additional *representations* are provided their metrics are
        computed, mapped through the appropriate evaluation section, and
        aggregated into the final verdict via the lattice.

        Args:
            morphism: The program to classify.
            representations: Optional extra representations to include.

        Returns:
            A ClassificationResult with the evaluation and all metrics.
        """
        if morphism.ast is None or not morphism.is_valid:
            return ClassificationResult(
                evaluation=EvaluationValue.INVALID,
                complexity_score=1.0,
                entropy_score=1.0,
                is_valid=False,
            )

        raw_complexity = calculate_cyclomatic_complexity(morphism.ast)
        raw_entropy = calculate_kolmogorov_proxy(morphism.source)
        complexity_assessment = classify_complexity(raw_complexity)
        entropy_assessment = classify_entropy(raw_entropy)

        complexity_score = normalize_complexity(raw_complexity)
        entropy_score = raw_entropy

        verdicts: dict[str, EvaluationValue] = {
            "complexity": complexity_assessment.evaluation,
            "entropy": entropy_assessment.evaluation,
        }

        representation_metrics: dict[str, dict[str, float]] = {}

        if representations:
            for rep in representations:
                raw = rep.metrics()
                representation_metrics[rep.name] = raw

                if rep.name == "depgraph":
                    verdicts.update(_depgraph_verdicts(raw))

        evaluation = self.omega.aggregate(verdicts)

        return ClassificationResult(
            evaluation=evaluation,
            complexity_score=float(complexity_score),
            entropy_score=entropy_score,
            is_valid=True,
            metrics={
                "raw_complexity": complexity_assessment.raw_score,
                "raw_entropy": entropy_assessment.raw_score,
                "node_count": morphism.ast.node_count,
                "depth": morphism.ast.depth,
                "complexity_evaluation": complexity_assessment.evaluation.name,
                "complexity_interpretation": complexity_assessment.interpretation,
                "entropy_evaluation": entropy_assessment.evaluation.name,
                "entropy_interpretation": entropy_assessment.interpretation,
            },
            representation_metrics=representation_metrics,
        )

    def combine(self, *values: EvaluationValue) -> EvaluationValue:
        """
        Combine multiple evaluation values using meet (∧).

        When evaluating a codebase with multiple files, the overall
        evaluation is the meet of all individual evaluations -- we're only as
        strong as our weakest link.
        """
        return self.omega.combine(*values)
