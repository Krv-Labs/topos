"""
Shared evaluation helpers used by the evaluate / assess / inspect tools.

Keeps the core pipeline in one place:
1. Build a ``ProgramMorphism``.
2. Attach CFG / academic PDG / CPG (always — they're derived from the
   morphism itself and require no external tooling).
3. Optionally attach a module-level ``ModuleDependencyGraph`` from GitNexus.
4. Call ``CharacteristicMorphism.classify_detailed``.

The classifier then assembles χ_S : P → Ω over the three generators
SIMPLE (← CFG), COMPOSABLE (← ModuleDependencyGraph), SECURE (← CPG).
"""

from __future__ import annotations

from pathlib import Path

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority
from topos.graphs.base import Representation
from topos.graphs.mdg.object import ModuleDependencyGraph

from .cache import dep_graph_for


def resolve_gitnexus_dir(
    override: str | Path | None, project_root: Path
) -> Path | None:
    """Return the gitnexus dir to use, or None if not available.

    Preference: explicit override > ``<project_root>/.gitnexus`` if it exists.
    """
    project_root = project_root.resolve()
    if override:
        path = Path(override).expanduser().resolve()
        try:
            path.relative_to(project_root)
        except ValueError:
            return None
        return path if path.exists() else None
    default = project_root / ".gitnexus"
    return default if default.exists() else None


def gitnexus_warnings(
    override: str | Path | None,
    project_root: Path,
    gitnexus_dir: Path | None,
    *,
    dep_graph_loaded: bool,
) -> list[str]:
    """Explain why COMPOSABLE is unavailable or risky."""
    project_root = project_root.resolve()
    warnings: list[str] = []
    if override:
        override_path = Path(override).expanduser().resolve()
        try:
            override_path.relative_to(project_root)
        except ValueError:
            return [
                "gitnexus_dir rejected — override must be inside TOPOS_MCP_FILE_ROOT. "
                f"Got: {override_path}"
            ]
        if not override_path.exists():
            return [
                "gitnexus_dir unavailable — override path does not exist. "
                f"Got: {override_path}"
            ]
    elif gitnexus_dir is None:
        return [
            "COMPOSABLE not scored — no .gitnexus directory found; run "
            "'topos depgraph generate' to score this generator."
        ]

    if gitnexus_dir is not None and not dep_graph_loaded:
        warnings.append(
            "COMPOSABLE not scored — .gitnexus exists but the dependency graph could "
            "not be loaded; re-run 'topos depgraph generate' and ensure GitNexus "
            "dependencies are installed."
        )
    if gitnexus_dir is not None:
        stale = _stale_gitnexus_warning(project_root, gitnexus_dir)
        if stale:
            warnings.append(stale)
    return warnings


def _stale_gitnexus_warning(project_root: Path, gitnexus_dir: Path) -> str | None:
    graph_mtime = _gitnexus_mtime(gitnexus_dir)
    if graph_mtime <= 0:
        return None
    head_mtime = _git_head_mtime(project_root)
    if head_mtime is None:
        return None
    if graph_mtime < head_mtime:
        return (
            "gitnexus index may be stale — .gitnexus is older than the latest "
            "git commit; run 'topos depgraph generate' before trusting COMPOSABLE."
        )
    return None


def _git_head_mtime(project_root: Path) -> float | None:
    git_dir = project_root / ".git"
    head = git_dir / "HEAD"
    try:
        head_text = head.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if head_text.startswith("ref: "):
        ref_path = git_dir / head_text.removeprefix("ref: ").strip()
        try:
            return ref_path.stat().st_mtime
        except OSError:
            return None
    try:
        return head.stat().st_mtime
    except OSError:
        return None


def _gitnexus_mtime(gitnexus_dir: Path) -> float:
    lbug = gitnexus_dir / "lbug"
    try:
        if lbug.exists():
            return lbug.stat().st_mtime
        return gitnexus_dir.stat().st_mtime
    except OSError:
        return 0.0


def load_dep_graph(
    gitnexus_dir: Path | None, target_file: str
) -> ModuleDependencyGraph | None:
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
    dep_graph: ModuleDependencyGraph | None = None,
) -> ClassificationResult:
    """Run the classifier with CFG/PDG/CPG plus an optional ModuleDependencyGraph."""
    reps: list[Representation] = _intrinsic_representations(morphism)
    if dep_graph is not None:
        reps.append(dep_graph)
    classifier = CharacteristicMorphism()
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
    generator is unreachable without a ModuleDependencyGraph.
    """
    morphism = ProgramMorphism(source=code, language=language)
    return classify_morphism(morphism, priority)


def classify_file(
    path: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
) -> tuple[ClassificationResult, ModuleDependencyGraph | None]:
    """Classify a file, attaching every available representation.

    Returns ``(result, dep_graph)`` so callers can cache the dep graph
    for subsequent proposed-code evaluations.
    """
    morphism = ProgramMorphism.from_file(path)
    dep_graph = load_dep_graph(gitnexus_dir, str(path))
    result = classify_morphism(morphism, priority, dep_graph)
    return result, dep_graph
