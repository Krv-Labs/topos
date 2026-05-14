"""
Shared evaluation helpers used by the evaluate / assess / inspect tools.

Keeps the core pipeline in one place:
1. Build a ``ProgramMorphism``.
2. Optionally attach a ``DependencyGraph`` (this is what was missing before —
   ``evaluate_file`` delegated to ``evaluate_code`` and dropped the filepath,
   so coupling never ran).
3. Call ``SubobjectClassifier.classify_detailed``.
"""

from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path

from topos.core.morphism import ProgramMorphism
from topos.graphs.base import Representation
from topos.graphs.pdg.graph import DependencyGraph
from topos.logic.omega import ClassificationResult, SubobjectClassifier
from topos.logic.policies.base import Priority
from .cache import dep_graph_for


def resolve_gitnexus_dir(
    override: str | Path | None, project_root: Path
) -> Path | None:
    """Return the gitnexus dir to use, or None if not available.

    Preference: explicit override > ``<project_root>/.gitnexus`` if it exists.
    """
    if override:
        path = Path(override).expanduser()
        return path if path.exists() else None
    default = project_root / ".gitnexus"
    return default if default.exists() else None


def load_dep_graph(
    gitnexus_dir: Path | None, target_file: str
) -> DependencyGraph | None:
    """Load the cached dep graph for a file, or None if not available."""
    if gitnexus_dir is None:
        return None
    try:
        return dep_graph_for(gitnexus_dir, target_file)
    except (FileNotFoundError, ImportError, OSError):
        # Silently degrade to AST-only evaluation when gitnexus can't load.
        return None


def classify_morphism(
    morphism: ProgramMorphism,
    priority: Priority,
    dep_graph: DependencyGraph | None = None,
) -> ClassificationResult:
    """Run the classifier with an optional DependencyGraph attached."""
    reps: Sequence[Representation] | None
    reps = [dep_graph] if dep_graph is not None else None
    classifier = SubobjectClassifier()
    return classifier.classify_detailed(
        morphism, representations=reps, priority=priority
    )


def classify_code_string(
    code: str, language: str, priority: Priority
) -> ClassificationResult:
    """Classify raw source (structural dimension only)."""
    morphism = ProgramMorphism(source=code, language=language)
    return classify_morphism(morphism, priority)


def classify_file(
    path: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
) -> tuple[ClassificationResult, DependencyGraph | None]:
    """Classify a file, attaching a DependencyGraph when available.

    Returns (result, dep_graph) so callers can cache the dep graph for
    subsequent proposed-code evaluations.
    """
    morphism = ProgramMorphism.from_file(path)
    # DependencyGraph matches on filepath; use the relative path as the target
    # so it lines up with what gitnexus indexed.
    dep_graph = load_dep_graph(gitnexus_dir, str(path))
    result = classify_morphism(morphism, priority, dep_graph)
    return result, dep_graph
