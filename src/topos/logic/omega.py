"""
Omega Module (The Subobject Classifier)
---------------------------------------
The 'Subobject Classifier' (Ω) is the soul of a Topos. It provides a
characteristic map that assigns a truth value from our Heyting Algebra
to any 'subobject' (piece of code).

Mathematical Inspiration:
    For every subobject 's' of 'X', there exists a unique morphism
    χ: X → Ω. We use this to classify 'commodity code' by its
    adherence to structural truth.

    In Set (the category of sets), Ω = {0, 1} and the characteristic
    function is simply membership. In our Topos of Programs, Ω is the
    TrustLattice, and the characteristic map evaluates code quality
    along multiple dimensions.

    The classifier combines metrics (complexity, entropy, distance)
    into a single truth value, acting as the 'judgment' of our system.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.logic.lattice import TruthLattice, TruthValue

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism


@dataclass
class ClassificationResult:
    """
    The result of classifying a program morphism.

    Contains the final truth value along with the individual
    metric scores that contributed to the classification.

    Attributes:
        truth_value: The final classification in the lattice.
        complexity_score: Normalized cyclomatic complexity (0-1).
        entropy_score: Normalized Kolmogorov proxy (0-1).
        is_valid: Whether the code parses successfully.
        metrics: Raw metric values for inspection.
    """

    truth_value: TruthValue
    complexity_score: float
    entropy_score: float
    is_valid: bool
    metrics: dict[str, float] = field(default_factory=dict)

    def __str__(self) -> str:
        return (
            f"Classification: {self.truth_value}\n"
            f"  Complexity: {self.complexity_score:.2f}\n"
            f"  Entropy: {self.entropy_score:.2f}\n"
            f"  Valid Syntax: {self.is_valid}"
        )


@dataclass
class SubobjectClassifier:
    """
    The Subobject Classifier (Ω) for the category of Programs.

    This is the core evaluation engine of topos. It maps program
    morphisms to truth values in the Heyting Algebra, implementing
    the characteristic map χ: Program → Ω.

    The classifier combines multiple metrics to determine where a
    piece of code falls in the lattice of trust:
    - INVALID: Fails to parse
    - HALLUCINATED: Parses but has extreme complexity or entropy
    - COMMODITY: Functional but with concerning metrics
    - VERIFIED: Well-structured, maintainable code

    Attributes:
        omega: The TruthLattice (our Ω).
        complexity_threshold: Max acceptable cyclomatic complexity.
        entropy_threshold: Max acceptable entropy ratio.

    Thresholds:
        These are configurable parameters that define the boundaries
        between truth values. They can be tuned based on project
        standards or organizational requirements.
    """

    omega: TruthLattice = field(default_factory=TruthLattice)
    complexity_threshold: float = 10.0
    entropy_threshold: float = 0.8

    def classify(self, morphism: ProgramMorphism) -> TruthValue:
        """
        Map a ProgramMorphism to a TruthValue in the Lattice.

        This is the core 'Evaluation' function of the library—the
        implementation of the characteristic map χ: X → Ω.

        The classification logic:
        1. If code doesn't parse → INVALID
        2. If complexity is extreme OR entropy is very high → HALLUCINATED
        3. If metrics are concerning but acceptable → COMMODITY
        4. If all metrics are good → VERIFIED

        Args:
            morphism: The program to classify.

        Returns:
            A TruthValue representing the code's position in the lattice.
        """
        result = self.classify_detailed(morphism)
        return result.truth_value

    def classify_detailed(self, morphism: ProgramMorphism) -> ClassificationResult:
        """
        Perform detailed classification with full metrics.

        Args:
            morphism: The program to classify.

        Returns:
            A ClassificationResult with the truth value and all metrics.
        """
        from topos.metrics.complexity import calculate_cyclomatic_complexity
        from topos.metrics.entropy import calculate_kolmogorov_proxy

        if morphism.ast is None or not morphism.is_valid:
            return ClassificationResult(
                truth_value=TruthValue.INVALID,
                complexity_score=1.0,
                entropy_score=1.0,
                is_valid=False,
            )

        raw_complexity = calculate_cyclomatic_complexity(morphism.ast)
        raw_entropy = calculate_kolmogorov_proxy(morphism.source)

        complexity_score = self._normalize_complexity(raw_complexity)
        entropy_score = raw_entropy

        truth_value = self._determine_truth_value(
            complexity_score=complexity_score,
            entropy_score=entropy_score,
        )

        return ClassificationResult(
            truth_value=truth_value,
            complexity_score=complexity_score,
            entropy_score=entropy_score,
            is_valid=True,
            metrics={
                "raw_complexity": raw_complexity,
                "raw_entropy": raw_entropy,
                "node_count": morphism.ast.node_count,
                "depth": morphism.ast.depth,
            },
        )

    def _normalize_complexity(self, raw_complexity: int) -> float:
        """
        Normalize cyclomatic complexity to [0, 1].

        Uses a sigmoid-like transformation centered at the threshold.
        """
        import math

        k = 0.3
        x = raw_complexity - self.complexity_threshold
        return 1 / (1 + math.exp(-k * x))

    def _determine_truth_value(
        self,
        complexity_score: float,
        entropy_score: float,
    ) -> TruthValue:
        """
        Combine metrics into a final truth value.

        Decision boundaries:
        - HALLUCINATED: complexity > 0.9 OR entropy > 0.95
        - COMMODITY: complexity > 0.5 OR entropy > 0.7
        - VERIFIED: otherwise
        """
        if complexity_score > 0.9 or entropy_score > 0.95:
            return TruthValue.HALLUCINATED

        if complexity_score > 0.5 or entropy_score > self.entropy_threshold:
            return TruthValue.COMMODITY

        return TruthValue.VERIFIED

    def combine(self, *values: TruthValue) -> TruthValue:
        """
        Combine multiple truth values using meet (∧).

        When evaluating a codebase with multiple files, the overall
        truth is the meet of all individual truths—we're only as
        strong as our weakest link.

        Args:
            values: Truth values to combine.

        Returns:
            The greatest lower bound of all values.
        """
        if not values:
            return self.omega.TOP

        result = values[0]
        for v in values[1:]:
            result = self.omega.meet(result, v)
        return result
