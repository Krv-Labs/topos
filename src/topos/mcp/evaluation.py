"""
Shared evaluation helpers used by the evaluate / assess / inspect tools.

Keeps the core pipeline in one place:
1. Build a ``ProgramMorphism``.
2. Attach CFG / academic PDG / CPG (always — they're derived from the
   morphism itself and require no external tooling).
3. Optionally attach a module-level ``DependencyGraph`` from GitNexus.
4. Call ``SubobjectClassifier.classify_detailed``.

The classifier then assembles χ_S : P → Ω over the three generators
SIMPLE (← CFG), COMPOSABLE (← DependencyGraph), SECURE (← CPG).
"""

from __future__ import annotations

from pathlib import Path

from topos.core.morphism import ProgramMorphism
from topos.graphs.base import Representation
from topos.graphs.mdg.object import DependencyGraph
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
        # Silently degrade when gitnexus can't load — CFG and CPG still run.
        return None


def _intrinsic_representations(
    morphism: ProgramMorphism,
) -> list[Representation]:
    """
    Build the three intrinsic representations derived from the UAST: CFG,
    academic PDG, CPG.  These require no external tooling so they are
    always attached.  Missing UAST (parse failure) yields an empty list.
    """
    reps: list[Representation] = []
    cfg = morphism.build_cfg()
    if cfg is not None:
        reps.append(cfg)
    pdg = morphism.build_pdg()
    if pdg is not None:
        reps.append(pdg)
    cpg = morphism.build_cpg()
    if cpg is not None:
        reps.append(cpg)
    return reps


def classify_morphism(
    morphism: ProgramMorphism,
    priority: Priority,
    dep_graph: DependencyGraph | None = None,
) -> ClassificationResult:
    """Run the classifier with CFG/PDG/CPG plus an optional DependencyGraph."""
    reps: list[Representation] = _intrinsic_representations(morphism)
    if dep_graph is not None:
        reps.append(dep_graph)
    classifier = SubobjectClassifier()
    return classifier.classify_detailed(
        morphism,
        representations=reps if reps else None,
        priority=priority,
    )


def classify_code_string(
    code: str, language: str, priority: Priority
) -> ClassificationResult:
    """
    Classify raw source.  CFG / PDG / CPG always run; the COMPOSABLE
    generator is unreachable without a DependencyGraph.
    """
    morphism = ProgramMorphism(source=code, language=language)
    return classify_morphism(morphism, priority)


def classify_file(
    path: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
) -> tuple[ClassificationResult, DependencyGraph | None]:
    """Classify a file, attaching every available representation.

    Returns ``(result, dep_graph)`` so callers can cache the dep graph
    for subsequent proposed-code evaluations.
    """
    morphism = ProgramMorphism.from_file(path)
    dep_graph = load_dep_graph(gitnexus_dir, str(path))
    result = classify_morphism(morphism, priority, dep_graph)
    return result, dep_graph


