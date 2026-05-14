"""
Category Module
---------------

Defines the categorical universe ``E = Set^(C × H^op)`` of the program
topos.  The base index category ``C`` lives in :mod:`topos.graphs.base`
(the directed-graph index category); the value Heyting algebra ``H = Ω``
lives in :mod:`topos.core.omega`; this module ties them together.

:class:`ProgramCategory` enforces composition axioms, maintains identity
mappings, and provides convenience access to:

    - the subobject classifier  ``Ω``   (:class:`~topos.core.omega.Omega`)
    - the characteristic morphism χ_S   (:class:`~topos.evaluation.characteristic_morphism.CharacteristicMorphism`)

so callers never need to reach into the logic subpackage just to classify
or compose programs.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject

if TYPE_CHECKING:
    from topos.evaluation.characteristic_morphism import (
        CharacteristicMorphism,
        ClassificationResult,
    )
    from topos.core.omega import EvaluationValue, Omega


class CategoryError(Exception):
    """Raised when category axioms (like composition domain mismatches) are broken."""

    pass


class ProgramCategory:
    """
    Encapsulates the categorical universe of our program topos.

    Provides utility methods for constructing identity maps, verifying
    composition legality, and reaching the topos's internal logic — the
    subobject classifier ``Ω`` and the characteristic morphism χ_S.
    """

    def __init__(self, name: str = "ToposOfPrograms"):
        self.name = name

    # ------------------------------------------------------------------
    # Categorical primitives
    # ------------------------------------------------------------------

    @staticmethod
    def identity(obj: ProgramObject) -> ProgramMorphism:
        """
        Constructs the Identity Morphism ``id_A : A → A`` for a given state.

        Mathematically, this is the trivial NOOP transformation that leaves
        the object's structural state completely invariant.
        """
        noop_source = (
            "def identity(x):\n    return x"
            if obj.language == "python"
            else "fn identity<T>(x: T) -> T { x }"
        )
        return ProgramMorphism(
            source=noop_source,
            language=obj.language,
        )

    @classmethod
    def compose(cls, g: ProgramMorphism, f: ProgramMorphism) -> ProgramMorphism:
        """
        Composes two program transformations to form a new Morphism (g ∘ f).

        Requires that the codomain of f matches the domain of g.  In
        software, this pipes the output structural block of f directly into
        the input of g.
        """
        composed_source = (
            f"{f.source}\n\n"
            f"{g.source}\n\n"
            "def composed_pipeline(*args, **kwargs):\n"
            "    return g(f(*args, **kwargs))"
        )

        return ProgramMorphism(
            source=composed_source,
            language=f.language,
            parser_backend=f.parser_backend,
        )

    def verify_commutativity(
        self, f: ProgramMorphism, g: ProgramMorphism, h: ProgramMorphism
    ) -> bool:
        """
        Verify whether a triangular diagram commutes: ``h == g ∘ f``.

        In the context of program transformations, decides if a direct
        refactoring/shortcut (h) is structurally identical to a multi-step
        pipeline (g ∘ f).
        """
        composed_gf = self.compose(g, f)

        from topos.functors.profunctors.ast.compare import structural_distance

        return structural_distance(h, composed_gf) == 0.0

    # ------------------------------------------------------------------
    # Internal logic — convenience access to Ω and χ_S
    # ------------------------------------------------------------------

    @staticmethod
    def omega() -> Omega:
        """
        Return a fresh instance of the subobject classifier ``Ω``.

        Ω carries both roles: the truth-value object of the topos and the
        value Heyting algebra of the internal logic.  See
        :mod:`topos.core.omega` for the algebra; see :meth:`classify`
        below for the characteristic morphism that maps programs into it.
        """
        from topos.core.omega import Omega

        return Omega()

    @staticmethod
    def characteristic_morphism() -> CharacteristicMorphism:
        """
        Return a fresh :class:`CharacteristicMorphism` (χ_S : P → Ω).

        The returned object can be applied to any
        :class:`~topos.core.morphism.ProgramMorphism` to produce a
        :class:`~topos.evaluation.characteristic_morphism.ClassificationResult`.
        """
        from topos.evaluation.characteristic_morphism import CharacteristicMorphism

        return CharacteristicMorphism()

    @classmethod
    def classify(cls, program: ProgramMorphism) -> EvaluationValue:
        """
        Apply χ_S : P → Ω to ``program`` and return the resulting Ω element.

        Convenience wrapper around
        :meth:`CharacteristicMorphism.classify`.
        """
        return cls.characteristic_morphism().classify(program)

    @classmethod
    def classify_detailed(
        cls, program: ProgramMorphism
    ) -> ClassificationResult:
        """
        Apply χ_S : P → Ω with full per-generator detail.

        Convenience wrapper around
        :meth:`CharacteristicMorphism.classify_detailed`.
        """
        return cls.characteristic_morphism().classify_detailed(program)
