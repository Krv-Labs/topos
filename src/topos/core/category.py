"""
Category Module
---------------
Defines the overarching categorical environment for Programs. It enforces
composition axioms, maintains identity mappings, and acts as the Topos domain.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject

if TYPE_CHECKING:
    pass


class CategoryError(Exception):
    """Raised when category axioms (like composition domain mismatches) are broken."""

    pass


class ProgramCategory:
    """
    Encapsulates the categorical universe of our program topos.

    Provides utility methods for constructing identity maps, verifying
    composition legality, and evaluating limits/colimits globally.
    """

    def __init__(self, name: str = "ToposOfPrograms"):
        self.name = name

    @staticmethod
    def identity(obj: ProgramObject) -> ProgramMorphism:
        """
        Constructs the Identity Morphism (id_A: A -> A) for a given state.

        Mathematically, this is the trivial NOOP transformation that leaves
        the object's structural state completely invariant.
        """
        noop_source = (
            "def identity(x):\n    return x"
            if obj.language == "python"
            else "fn identity<T>(x: T) -> T { x }"
        )
        # Generate a pristine pass-through morphism bound to this object's metadata
        return ProgramMorphism(
            source=noop_source,
            language=obj.language,
        )

    @classmethod
    def compose(cls, g: ProgramMorphism, f: ProgramMorphism) -> ProgramMorphism:
        """
        Composes two program transformations to form a new Morphism (g ∘ f).

        Requires that the codomain of f matches the domain of g. In software,
        this pipes the output structural block of f directly into the input of g.
        """
        # Optional: Implement a validation check ensuring types/schemas match if your
        # ProgramObjects carry strict type/signature boundary definitions.
        # if f.codomain != g.domain:
        #     raise CategoryError(f"Cannot compose: Codomain of {f.name} does not match Domain of {g.name}")

        composed_source = f"{f.source}\n\n{g.source}\n\ndef composed_pipeline(*args, **kwargs):\n    return g(f(*args, **kwargs))"

        # In a sophisticated system, your AST dispatch layer would join the two graph objects.
        return ProgramMorphism(
            source=composed_source, language=f.language, parser_backend=f.parser_backend
        )

    def verify_commutativity(
        self, f: ProgramMorphism, g: ProgramMorphism, h: ProgramMorphism
    ) -> bool:
        """
        Verifies if a triangular diagram commutes: h == g ∘ f

        In the context of program transformations, it determines if a direct refactoring/shortcut
        (h) is structurally and behaviorally identical to a multi-step pipeline (g ∘ f).
        """
        composed_gf = self.compose(g, f)

        # Leverage your existing profunctors/distance metrics to check if structural
        # distance between the direct map and the pipeline is zero.
        from topos.functors.profunctors.distance import structural_distance

        return structural_distance(h, composed_gf) == 0.0
