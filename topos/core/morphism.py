"""ProgramMorphism — thin wrapper over the Rust engine."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from topos.topos_functors import ProgramMorphism

_orig_from_file = ProgramMorphism.from_file


def _from_file(
    filepath: str | Path,
    language: str | None = None,
    parser_backend: Any = "hybrid",
) -> ProgramMorphism:
    del parser_backend
    lang = language or "python"
    return _orig_from_file(str(filepath), lang)


ProgramMorphism.from_file = staticmethod(_from_file)  # type: ignore[method-assign]


def _classify(self: ProgramMorphism):
    """Evaluate this morphism using the Subobject Classifier."""
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

    return CharacteristicMorphism().classify(self)


ProgramMorphism.classify = _classify  # type: ignore[method-assign]

__all__ = ["ProgramMorphism"]
