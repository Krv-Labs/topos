"""
Topos: Category-Theoretic Code Quality Evaluation

Treating programs as morphisms in a world of commodity code.
Building the subobject classifier for rigorous program evaluations.

This library applies concepts from Topos theory to evaluate code quality,
moving beyond simple numeric metrics to a Heyting Algebra of evaluation
values that can express partial confidence about program quality and
maintainability.
"""

from __future__ import annotations

import importlib
from typing import Any

from topos._version import __version__

__all__ = [
    "__version__",
    # Categorical primitives
    "ProgramCategory",
    "ProgramMorphism",
    "ProgramObject",
    # Internal logic of the topos
    "Omega",
    "EvaluationValue",
    "verdict_from_generators",
    "CharacteristicMorphism",
    "ClassificationResult",
    # Translational functors (representations)
    "Representation",
    "ASTRepresentation",
    "ControlFlowGraph",
    "ProgramDependenceGraph",
    "ModuleDependencyGraph",
    "CodePropertyGraph",
]

_LAZY_SUBMODULES = frozenset({"core", "graphs", "evaluation"})

_LAZY_EXPORTS: dict[str, tuple[str, str]] = {
    "ProgramCategory": ("topos.core.category", "ProgramCategory"),
    "ProgramMorphism": ("topos.core.morphism", "ProgramMorphism"),
    "ProgramObject": ("topos.core.object", "ProgramObject"),
    "Omega": ("topos.core.omega", "Omega"),
    "EvaluationValue": ("topos.core.omega", "EvaluationValue"),
    "verdict_from_generators": ("topos.core.omega", "verdict_from_generators"),
    "CharacteristicMorphism": (
        "topos.evaluation.characteristic_morphism",
        "CharacteristicMorphism",
    ),
    "ClassificationResult": (
        "topos.evaluation.characteristic_morphism",
        "ClassificationResult",
    ),
    "Representation": ("topos.graphs.base", "Representation"),
    "ASTRepresentation": ("topos.graphs.ast.object", "ASTRepresentation"),
    "ControlFlowGraph": ("topos.graphs.cfg.object", "ControlFlowGraph"),
    "ProgramDependenceGraph": ("topos.graphs.pdg.object", "ProgramDependenceGraph"),
    "ModuleDependencyGraph": ("topos.graphs.mdg.object", "ModuleDependencyGraph"),
    "CodePropertyGraph": ("topos.graphs.cpg.object", "CodePropertyGraph"),
}


def __dir__() -> list[str]:
    return sorted([*__all__, *_LAZY_SUBMODULES])


def __getattr__(name: str) -> Any:
    if name in _LAZY_SUBMODULES:
        return importlib.import_module(f"topos.{name}")
    if name not in _LAZY_EXPORTS:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    module_name, attr_name = _LAZY_EXPORTS[name]
    module = importlib.import_module(module_name)
    value = getattr(module, attr_name)
    globals()[name] = value
    return value
