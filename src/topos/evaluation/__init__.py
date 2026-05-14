"""
Evaluation Module
-----------------

How Topos *evaluates* programs — the bridge from raw measurements to
verdicts in the topos's truth-value object ``Ω``.

The categorical primitive itself — Ω, the subobject classifier — is part
of the topos and therefore lives in :mod:`topos.core.omega`.  This
package contains only the *mapping into* Ω:

    :mod:`topos.evaluation.characteristic_morphism`
        ``χ_S : P → Ω`` — the natural transformation that assigns each
        program morphism its verdict in Ω.  Implements
        :class:`CharacteristicMorphism` (the map) and
        :class:`ClassificationResult` (its image on a single program).

    :mod:`topos.evaluation.policies`
        Policy translators ``Φᵢ : ℝ → Ω`` — one per quality generator.
        Each ``Φᵢ`` converts raw real-valued probe outputs (cyclomatic
        complexity, coupling, taint-flow count, …) into a thresholded
        truth value for one generator.

Conceptually::

    raw probes (ℝ)  ──Φ─▶  truth values  ──χ_S─▶  Ω element

Together, ``Φ`` and ``χ_S`` form the *decision layer* of the topos:
they turn measurements into structured verdicts.  The internal logic
itself (meet, join, implies, negation) lives on Ω in
:mod:`topos.core.omega` — those operations are properties of the topos,
not of the evaluator.

The decision layer is intuitionistic: partial evidence across the three
generators is allowed; the law of excluded middle does *not* hold.
"""

from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)

__all__ = [
    "CharacteristicMorphism",
    "ClassificationResult",
]
