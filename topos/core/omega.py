"""
Omega — The Subobject Classifier of the Topos
=============================================

This module *is* ``Ω``, the subobject classifier of the topos
``E = Set^(C × H^op)``.  Equivalently, it is the value Heyting algebra
``H = H(G_qual)``, the **free Heyting algebra** on the finite set of
quality generators

    G_qual = { SIMPLE, COMPOSABLE, SECURE }

In a topos the subobject classifier object and the internal-logic Heyting
algebra coincide — Ω carries both roles.  The *characteristic morphism*
``χ_S : P → Ω`` that maps a program into Ω lives in
:mod:`topos.evaluation.characteristic_morphism`; this file holds only the
algebra itself (objects, ordering, lattice operations).

The carrier of Ω is the 8-element poset of all subsets of ``G_qual``:

::

                          IDEAL  (top, ⊤ = SIMPLE ∧ COMPOSABLE ∧ SECURE)
                         /  |  \\
                        /   |   \\
                       /    |    \\
        SIMPLE_COMPOSABLE  SIMPLE_SECURE  COMPOSABLE_SECURE
              |  \\  /             \\  /  |
              |   \\/               \\/   |
              |   /\\               /\\   |
              |  /  \\             /  \\  |
            SIMPLE   COMPOSABLE         SECURE
                       \\    |    /
                        \\   |   /
                         \\  |  /
                          SLOP  (bottom, ⊥)

The three generators are pairwise incomparable: ``leq(SIMPLE, COMPOSABLE)``
is ``False`` in both directions.  Meets are intersections of the satisfied
generator sets; ``meet(SIMPLE, COMPOSABLE) == SIMPLE_COMPOSABLE`` adds a
generator; ``meet(SIMPLE_COMPOSABLE, SECURE) == IDEAL``.

The ordering is the *partial* order of *satisfied-generator inclusion*: a
verdict ``a`` is ≤ ``b`` iff the set of generators ``a`` satisfies is a
*superset* of the set ``b`` satisfies.  Top (``IDEAL``) satisfies every
generator; bottom (``SLOP``) satisfies none.  This is the order required by
the math spec (section 1, "reverse metric/constraint inclusion"): adding a
satisfied constraint moves the verdict *down* toward ``IDEAL``.

The implementation uses an explicit cover relation rather than an integer
ordering — singletons (``SIMPLE``, ``COMPOSABLE``, ``SECURE``) are pairwise
incomparable, so the Hasse diagram is a 3-cube, not a chain.  ``meet`` /
``join`` / ``implies`` / ``negation`` are computed generically from the
cover, so this engine works for arbitrary finite Heyting algebras.

Categorical / Pythonic names::

    Math               Python
    --------------     -----------------------------------------
    Ω                  Omega                         (this class)
    elements of Ω      EvaluationValue                (the enum)
    ⊤                  EvaluationValue.IDEAL          / Omega.TOP
    ⊥                  EvaluationValue.SLOP           / Omega.BOTTOM
    χ_S : P → Ω        CharacteristicMorphism         (sibling module)

The top is ``IDEAL`` — the joint-satisfaction of all generators.  The
bottom is ``SLOP``, the unconstrained universe.
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from typing import ClassVar

from topos.topos_functors import (
    EvaluationValue,
    all_evaluation_values,
    verdict_from_generators,
)

__all__ = [
    "EvaluationValue",
    "all_evaluation_values",
    "verdict_from_generators",
    "Omega",
]
# ---------------------------------------------------------------------------
# Cover relation for the 3-cube
# ---------------------------------------------------------------------------
# Each successor *adds* one satisfied generator (turns one bit on), which in
# this order moves *down* toward IDEAL.  We list ``cover[a] = [b, ...]`` to
# mean "b is an immediate successor of a (i.e. a is covered by b, a ≤ b)".

_CUBE_COVER: dict[EvaluationValue, list[EvaluationValue]] = {
    EvaluationValue.SLOP: [
        EvaluationValue.SIMPLE,
        EvaluationValue.COMPOSABLE,
        EvaluationValue.SECURE,
    ],
    EvaluationValue.SIMPLE: [
        EvaluationValue.SIMPLE_COMPOSABLE,
        EvaluationValue.SIMPLE_SECURE,
    ],
    EvaluationValue.COMPOSABLE: [
        EvaluationValue.SIMPLE_COMPOSABLE,
        EvaluationValue.COMPOSABLE_SECURE,
    ],
    EvaluationValue.SECURE: [
        EvaluationValue.SIMPLE_SECURE,
        EvaluationValue.COMPOSABLE_SECURE,
    ],
    EvaluationValue.SIMPLE_COMPOSABLE: [EvaluationValue.IDEAL],
    EvaluationValue.SIMPLE_SECURE: [EvaluationValue.IDEAL],
    EvaluationValue.COMPOSABLE_SECURE: [EvaluationValue.IDEAL],
    EvaluationValue.IDEAL: [],
}


@dataclass
class Omega:
    """
    Ω — the subobject classifier object of the program topos.

    In the topos ``E = Set^(C × H^op)`` the subobject classifier coincides
    with the value Heyting algebra ``H(G_qual)``.  This class carries both
    roles: it is the truth-value object whose elements (``EvaluationValue``)
    are the verdicts a program can receive, *and* the Heyting algebra whose
    operations (meet, join, implies, negation) give the internal logic of
    the topos.

    Encodes the 3-cube Hasse diagram via an explicit cover relation.  All
    lattice operations are computed generically from the cover; no change
    is needed if the algebra is later extended with additional generators
    or modified by quotient relations.

    Class Attributes:
        BOTTOM: The least element (⊥ = SLOP)
        TOP:    The greatest element (⊤ = IDEAL)
    """

    BOTTOM: ClassVar[EvaluationValue] = EvaluationValue.SLOP
    TOP: ClassVar[EvaluationValue] = EvaluationValue.IDEAL

    DEFAULT_COVER: ClassVar[dict[EvaluationValue, list[EvaluationValue]]] = _CUBE_COVER

    # Direct cover relations: value -> immediate successors.
    cover: dict[EvaluationValue, list[EvaluationValue]] = field(default_factory=dict)
    _closure: dict[tuple[EvaluationValue, EvaluationValue], bool] = field(
        init=False,
        repr=False,
        default_factory=dict,
    )

    def __post_init__(self) -> None:
        if not self.cover:
            self._set_default_cover()

        self._closure = {}
        for value in all_evaluation_values():
            for dominated in self._collect_dominates(value):
                self._closure[(value, dominated)] = True
            self._closure[(value, value)] = True

    @classmethod
    def from_cover_relation(
        cls, cover: Mapping[EvaluationValue, Iterable[EvaluationValue]]
    ) -> Omega:
        """
        Construct a lattice from direct cover relations.

        Args:
            cover: Mapping from each value to its immediate successors.
        """
        normalized_cover = {
            value: list(successors) for value, successors in cover.items()
        }
        return cls(cover=normalized_cover)

    def _set_default_cover(self) -> None:
        """Safe fallback when no cover is provided."""
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
        """Lattice ordering: a ≤ b."""
        return self._closure.get((a, b), False)

    def meet(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        The 'And' operation (Greatest Lower Bound).

        For the free Heyting algebra on quality generators, this is the
        intersection of satisfied-generator sets.
        """
        return self._resolve_bounds(a, b, maximize=False)

    def join(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        The 'Or' operation (Least Upper Bound).

        For the free Heyting algebra on quality generators, this is the
        union of satisfied-generator sets (i.e. the most-specific verdict
        that *both* a and b dominate).
        """
        return self._resolve_bounds(a, b, maximize=True)

    def implies(self, a: EvaluationValue, b: EvaluationValue) -> EvaluationValue:
        """
        Intuitionistic implication (→).

        a → b is the largest x such that a ∧ x ≤ b.
        """
        candidates = [
            x for x in all_evaluation_values() if self.leq(self.meet(a, x), b)
        ]
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
        Aggregate evaluation values via meet.

        Multi-file rollup is exactly this meet: a generator is satisfied
        across a codebase iff it is satisfied for every file.
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
        """Combine multiple evaluation values using meet (∧)."""
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
        all_values = tuple(all_evaluation_values())
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
        """Select minimal or maximal elements under the partial order."""
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
