"""
Lattice Module (Heyting Algebra)
--------------------------------
Implements the 'Trust Lattice'. In intuitionistic logic, truth is not
merely binary {0, 1}, but a collection of 'Stages of Truth'.

Mathematical Inspiration:
    A Heyting Algebra is a bounded lattice that acts as the internal logic
    of a Topos. It supports the 'implies' operation (internal hom) and
    does not necessarily satisfy the Law of Excluded Middle (A ∨ ¬A).

    In 'topos', code can be:
    - ⊥ (Bottom): Syntactically invalid.
    - Hallucinated: Correct syntax, zero logical value.
    - Commodity: Runs, but lacks structural integrity.
    - ⊤ (Top): Verified, maintainable, and human-aligned.

    The lattice forms a total order: ⊥ < HALLUCINATED < COMMODITY < ⊤

    Operations:
    - meet (∧): Greatest Lower Bound - the pessimistic combination
    - join (∨): Least Upper Bound - the optimistic combination
    - implies (→): Relative pseudo-complement - intuitionistic implication
    - not (¬): Pseudo-complement - intuitionistic negation
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum
from functools import total_ordering
from typing import ClassVar


@total_ordering
class TruthValue(IntEnum):
    """
    The stages of code integrity in our Heyting Algebra.

    This enumeration defines the four truth values that form our lattice,
    ordered from bottom (⊥) to top (⊤).

    Values:
        INVALID: ⊥ - Code that fails to parse. Syntactically broken.
        HALLUCINATED: Parses correctly but is logically vacuous.
                      Typical of naive LLM output that 'looks like' code.
        COMMODITY: Functional code that runs, but lacks structural integrity.
                   May have high complexity, poor organization, or debt.
        VERIFIED: ⊤ - Maintainable, well-structured, human-aligned code.
                  The ideal we aspire to.
    """

    INVALID = 0  # ⊥ (Bottom)
    HALLUCINATED = 1
    COMMODITY = 2
    VERIFIED = 3  # ⊤ (Top)

    @property
    def symbol(self) -> str:
        """Unicode symbol representation."""
        symbols = {
            TruthValue.INVALID: "⊥",
            TruthValue.HALLUCINATED: "○",
            TruthValue.COMMODITY: "◐",
            TruthValue.VERIFIED: "⊤",
        }
        return symbols[self]

    @property
    def description(self) -> str:
        """Human-readable description of this truth value."""
        descriptions = {
            TruthValue.INVALID: "Syntactically invalid code",
            TruthValue.HALLUCINATED: "Parses but logically vacuous",
            TruthValue.COMMODITY: "Functional but structurally weak",
            TruthValue.VERIFIED: "Verified, maintainable, and aligned",
        }
        return descriptions[self]

    def __str__(self) -> str:
        return f"{self.symbol} {self.name}"


@dataclass
class TruthLattice:
    """
    The Heyting Algebra of code trust.

    This class implements the lattice operations over TruthValue,
    providing the algebraic structure needed for intuitionistic
    reasoning about code quality.

    The lattice is a total order (linear chain):
        INVALID < HALLUCINATED < COMMODITY < VERIFIED

    This simplifies the algebra while still capturing the essential
    gradations of code quality.

    Class Attributes:
        BOTTOM: The least element (⊥ = INVALID)
        TOP: The greatest element (⊤ = VERIFIED)
    """

    BOTTOM: ClassVar[TruthValue] = TruthValue.INVALID
    TOP: ClassVar[TruthValue] = TruthValue.VERIFIED

    def meet(self, a: TruthValue, b: TruthValue) -> TruthValue:
        """
        The 'And' operation (Greatest Lower Bound).

        In our linear lattice, meet is simply the minimum.

        Args:
            a: First truth value.
            b: Second truth value.

        Returns:
            The greatest lower bound of a and b.

        Example:
            meet(COMMODITY, VERIFIED) = COMMODITY
            meet(INVALID, HALLUCINATED) = INVALID
        """
        return TruthValue(min(a.value, b.value))

    def join(self, a: TruthValue, b: TruthValue) -> TruthValue:
        """
        The 'Or' operation (Least Upper Bound).

        In our linear lattice, join is simply the maximum.

        Args:
            a: First truth value.
            b: Second truth value.

        Returns:
            The least upper bound of a and b.

        Example:
            join(COMMODITY, VERIFIED) = VERIFIED
            join(INVALID, HALLUCINATED) = HALLUCINATED
        """
        return TruthValue(max(a.value, b.value))

    def implies(self, a: TruthValue, b: TruthValue) -> TruthValue:
        """
        Intuitionistic implication (→).

        The relative pseudo-complement: a → b is the largest x such that
        a ∧ x ≤ b. In a linear order, this simplifies to:
            a → b = ⊤  if a ≤ b
            a → b = b  otherwise

        Args:
            a: Antecedent truth value.
            b: Consequent truth value.

        Returns:
            The truth value of 'a implies b'.

        Example:
            implies(COMMODITY, VERIFIED) = VERIFIED
            implies(VERIFIED, COMMODITY) = COMMODITY
        """
        if a <= b:
            return self.TOP
        return b

    def negation(self, a: TruthValue) -> TruthValue:
        """
        Intuitionistic negation (¬).

        Defined as: ¬a = a → ⊥

        In intuitionistic logic, ¬¬a ≠ a in general.
        Only ⊥ has a non-trivial negation (¬⊥ = ⊤).

        Args:
            a: Truth value to negate.

        Returns:
            The pseudo-complement of a.

        Example:
            negation(INVALID) = VERIFIED  (¬⊥ = ⊤)
            negation(anything else) = INVALID  (¬x = ⊥ for x > ⊥)
        """
        return self.implies(a, self.BOTTOM)

    def leq(self, a: TruthValue, b: TruthValue) -> bool:
        """
        Lattice ordering: a ≤ b.

        Args:
            a: First truth value.
            b: Second truth value.

        Returns:
            True if a is less than or equal to b in the lattice.
        """
        return a.value <= b.value

    def equivalent(self, a: TruthValue, b: TruthValue) -> bool:
        """
        Check if two truth values are equivalent.

        In a Heyting Algebra, a ↔ b iff (a → b) ∧ (b → a) = ⊤

        Args:
            a: First truth value.
            b: Second truth value.

        Returns:
            True if a and b are equivalent.
        """
        return self.meet(self.implies(a, b), self.implies(b, a)) == self.TOP
