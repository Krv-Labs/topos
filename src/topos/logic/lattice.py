"""
Lattice Module (Heyting Algebra)
--------------------------------
Implements our configured evaluation lattice. In intuitionistic logic,
evaluation is not merely binary {0, 1}, but can represent partial evidence
across multiple stages.

Mathematical Inspiration:
    A Heyting Algebra is a bounded lattice that acts as the internal logic
    of a Topos. It supports the 'implies' operation (internal hom) and
    does not necessarily satisfy the Law of Excluded Middle (A ∨ ¬A).

The lattice structure is intentionally non-total. In a lattice this means some
evaluation values are incomparable until combined through meet/join.
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from enum import IntEnum
from typing import ClassVar


class EvaluationValue(IntEnum):
    """
    The stages of code quality in our Heyting Algebra.

    This enumeration defines the six evaluation values that form our lattice,
    ordered from bottom (⊥) to top (⊤) through a partial order.

    Each label describes a *structural observation* about the code — what the
    metrics are detecting — rather than an abstract quality judgment.  The two
    middle pairs (ENTANGLED/COUPLED and COMPLEX/STABLE) correspond to the
    project's two evaluation pillars: **complexity** (AST-derived) and
    **coupling** (dependency-graph-derived).

    Values:
        BROKEN:     ⊥ - Parse failure (pre-evaluation) or lattice bottom within
                    a dimension. When returned by a dimension, it means the
                    structural signal for that axis is at the worst possible
                    evaluation (not necessarily a syntax error).
        ENTANGLED:  Extreme structural or coupling pathology. Complexity ≥ 40
                    or entropy ≥ 0.95 — the code is too tangled to reason about.
        COUPLED:    Anomalous structural signal. Significantly off-range entropy,
                    high-range complexity ([24, 40)), or tight coupling that
                    makes the module brittle.
        COMPLEX:    Elevated branching or off-range entropy beyond what the task
                    warrants, but still manageable.
        STABLE:     Working code — structurally sound with minor concerns.
                    Recoverable with targeted refactoring.
        SOUND:      ⊤ - Clean, maintainable, and appropriately scoped.
    """

    BROKEN = 0      # ⊥ (Bottom)
    ENTANGLED = 1
    COUPLED = 2
    COMPLEX = 3
    STABLE = 4
    SOUND = 5       # ⊤ (Top)

    @property
    def symbol(self) -> str:
        """Unicode symbol representation."""
        symbols = {
            EvaluationValue.BROKEN: "⊥",
            EvaluationValue.ENTANGLED: "○",
            EvaluationValue.COUPLED: "◑",
            EvaluationValue.COMPLEX: "◒",
            EvaluationValue.STABLE: "◐",
            EvaluationValue.SOUND: "⊤",
        }
        return symbols[self]

    @property
    def description(self) -> str:
        """Human-readable description of this evaluation value."""
        descriptions = {
            EvaluationValue.BROKEN: "Structurally broken; cannot be evaluated",
            EvaluationValue.ENTANGLED: "Extreme structural or coupling pathology",
            EvaluationValue.COUPLED: "Significant anomaly; tight coupling or brittle structure",
            EvaluationValue.COMPLEX: "More complex than the task warrants",
            EvaluationValue.STABLE: "Working code; structurally sound with minor concerns",
            EvaluationValue.SOUND: "Clean, maintainable, appropriately scoped",
        }
        return descriptions[self]

    def __str__(self) -> str:
        return f"{self.symbol} {self.name}"


@dataclass
class EvaluationLattice:
    """
    The Heyting Algebra of program evaluation.

    This class implements lattice operations over EvaluationValue,
    using an explicit partial order relation instead of assuming a total
    chain.

    Class Attributes:
        BOTTOM: The least element (⊥ = BROKEN)
        TOP: The greatest element (⊤ = SOUND)
    """

    BOTTOM: ClassVar[EvaluationValue] = EvaluationValue.BROKEN
    TOP: ClassVar[EvaluationValue] = EvaluationValue.SOUND

    DEFAULT_COVER: ClassVar[dict[EvaluationValue, list[EvaluationValue]]] = {
        EvaluationValue.BROKEN: [
            EvaluationValue.ENTANGLED,
            EvaluationValue.COUPLED,
            EvaluationValue.COMPLEX,
            EvaluationValue.STABLE,
        ],
        EvaluationValue.ENTANGLED: [EvaluationValue.SOUND],
        EvaluationValue.COUPLED: [EvaluationValue.STABLE],
        EvaluationValue.COMPLEX: [EvaluationValue.STABLE],
        EvaluationValue.STABLE: [EvaluationValue.SOUND],
        EvaluationValue.SOUND: [],
    }

    # Direct cover relations: value -> immediate successors.
    cover: dict[EvaluationValue, list[EvaluationValue]] = field(default_factory=dict)
    _closure: dict[tuple[EvaluationValue, EvaluationValue], bool] = field(
        init=False,
        repr=False,
        default_factory=dict,
    )

    def __post_init__(self) -> None:
        if not self.cover:
            self._set_total_chain()

        self._closure = {}
        for value in EvaluationValue:
            for dominated in self._collect_dominates(value):
                self._closure[(value, dominated)] = True
            self._closure[(value, value)] = True

    @classmethod
    def from_cover_relation(
        cls, cover: Mapping[EvaluationValue, Iterable[EvaluationValue]]
    ) -> EvaluationLattice:
        """
        Construct a lattice from direct cover relations.

        Args:
            cover: Mapping from each value to its immediate successors.
        """
        normalized_cover = {
            value: list(successors) for value, successors in cover.items()
        }
        return cls(cover=normalized_cover)

    def _set_total_chain(self) -> None:
        """
        Safe fallback when no cover is provided.

        Uses DEFAULT_COVER so existing callers still get deterministic behavior.
        """
        self.cover = self.DEFAULT_COVER

    def _collect_dominates(self, value: EvaluationValue) -> set[EvaluationValue]:
        stack = list(self.cover.get(value, ()))
        visited: set[EvaluationValue] = set()

        while stack:
            current = stack.pop()
            if current in visited:
                continue
            visited.add(current)
            stack.extend(self.cover.get(current, ()))

        return visited

    def leq(self, a: EvaluationValue, b: EvaluationValue) -> bool:
        """
        Lattice ordering: a ≤ b.
        """
        return self._closure.get((a, b), False)

    def meet(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        The 'And' operation (Greatest Lower Bound).

        In the general lattice this is the greatest value that is below
        both a and b.
        """
        return self._resolve_bounds(a, b, maximize=False)

    def join(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        The 'Or' operation (Least Upper Bound).

        In the general lattice this is the least value above both a and b.
        """
        return self._resolve_bounds(a, b, maximize=True)

    def implies(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        Intuitionistic implication (→).

        a → b is the largest x such that a ∧ x ≤ b.
        In total orders this simplifies, but here we compute it directly.
        """
        candidates = [x for x in EvaluationValue if self.leq(self.meet(a, x), b)]
        extrema = self._select_extrema(candidates, minimal=False)
        if len(extrema) != 1:
            raise ValueError(f"Cannot compute implies for {a} and {b}")
        return extrema[0]

    def negation(self, a: EvaluationValue) -> EvaluationValue:
        """Intuitionistic negation (¬), i.e. a → ⊥."""
        return self.implies(a, self.BOTTOM)

    def aggregate(
        self,
        metric_evaluations: Iterable[EvaluationValue] | Mapping[str, EvaluationValue],
    ) -> EvaluationValue:
        """
        Aggregate evaluation values contributed by independent metrics.

        This keeps the lattice neutral to metric-specific thresholds:
        each metric defines its own verdict; the lattice chooses the combined
        verdict.
        """
        values = (
            metric_evaluations.values()
            if isinstance(metric_evaluations, Mapping)
            else metric_evaluations
        )
        iterator = iter(values)
        try:
            result = next(iterator)
        except StopIteration:
            return self.TOP

        for value in iterator:
            result = self.meet(result, value)
        return result

    def combine(self, *values: EvaluationValue) -> EvaluationValue:
        """
        Combine multiple evaluation values using meet (∧).

        When evaluating a codebase with multiple files, the overall
        evaluation is the meet of all individual evaluations.
        """
        return self.aggregate(values)

    def equivalent(self, a: EvaluationValue, b: EvaluationValue) -> bool:
        """
        Check if two evaluation values are equivalent.

        In a Heyting Algebra, a ↔ b iff (a → b) ∧ (b → a) = ⊤
        """
        return self.meet(self.implies(a, b), self.implies(b, a)) == self.TOP

    def _resolve_bounds(
        self,
        a: EvaluationValue,
        b: EvaluationValue,
        *,
        maximize: bool,
    ) -> EvaluationValue:
        all_values = tuple(EvaluationValue)
        if maximize:
            bounds = [v for v in all_values if self.leq(a, v) and self.leq(b, v)]
            candidates = self._select_extrema(bounds, minimal=True)
        else:
            bounds = [v for v in all_values if self.leq(v, a) and self.leq(v, b)]
            candidates = self._select_extrema(bounds, minimal=False)

        if len(candidates) != 1:
            raise ValueError(
                f"Cannot compute {'join' if maximize else 'meet'} for {a} and {b}"
            )
        return candidates[0]

    def _select_extrema(
        self, candidates: list[EvaluationValue], *, minimal: bool
    ) -> list[EvaluationValue]:
        """
        Select minimal or maximal elements from candidates under the partial order.
        """
        if not candidates:
            return []
        return [
            c
            for c in candidates
            if not any(
                c != other and (self.leq(other, c) if minimal else self.leq(c, other))
                for other in candidates
            )
        ]
