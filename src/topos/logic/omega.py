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

    The classifier combines metrics (complexity, entropy, distance)
    into a single evaluation value, acting as the judgment of our system.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.policies import (
    build_evaluation_lattice,
    classify_complexity,
    classify_entropy,
    normalize_complexity,
)
from topos.metrics.complexity import calculate_cyclomatic_complexity
from topos.metrics.entropy import calculate_kolmogorov_proxy

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism


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
    """

    evaluation: EvaluationValue
    complexity_score: float
    entropy_score: float
    is_valid: bool
    metrics: dict[str, object] = field(default_factory=dict)

    def __str__(self) -> str:
        return (
            f"Classification: {self.evaluation}\n"
            f"  Complexity: {self.complexity_score:.2f}\n"
            f"  Entropy: {self.entropy_score:.2f}\n"
            f"  Valid Syntax: {self.is_valid}"
        )


@dataclass
class SubobjectClassifier:
    """
    The Subobject Classifier (Ω) for the category of Programs.

    This is the core evaluation engine of topos. It maps program
    morphisms to evaluation values in the Heyting Algebra, implementing
    the characteristic map χ: Program → Ω.

    The classifier combines metric verdicts to determine where a
    piece of code falls in a six-point lattice:
    INVALID < HALLUCINATED < VERIFIED and
    INVALID < {NOISY, WEAK, COMMODITY} with NOISY/WEAK < COMMODITY < VERIFIED.

    Attributes:
        omega: The EvaluationLattice (our Ω).
    """

    omega: EvaluationLattice = field(default_factory=build_evaluation_lattice)

    def classify(self, morphism: ProgramMorphism) -> EvaluationValue:
        """
        Map a ProgramMorphism to an EvaluationValue in the lattice.

        This is the core 'Evaluation' function of the library—the
        implementation of the characteristic map χ: X → Ω.

        The classification logic:
        1. If code doesn't parse → INVALID
        2. Otherwise run metric-level classifiers from `complexity` and `entropy`.
        3. Combine their lattice evaluations with meet to surface nuanced grades.

        Args:
            morphism: The program to classify.

        Returns:
            An EvaluationValue representing the code's position in the lattice.
        """
        result = self.classify_detailed(morphism)
        return result.evaluation

    def classify_detailed(self, morphism: ProgramMorphism) -> ClassificationResult:
        """
        Perform detailed classification with full metrics.

        Args:
            morphism: The program to classify.

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
        evaluation = self.omega.aggregate(
            {
                "complexity": complexity_assessment.evaluation,
                "entropy": entropy_assessment.evaluation,
            }
        )

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
        )

    def combine(self, *values: EvaluationValue) -> EvaluationValue:
        """
        Combine multiple evaluation values using meet (∧).

        When evaluating a codebase with multiple files, the overall
        evaluation is the meet of all individual evaluations—we're only as
        strong as our weakest link.
        """
        return self.omega.combine(*values)
