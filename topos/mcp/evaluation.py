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

from dataclasses import dataclass
from pathlib import Path

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority
from topos.graphs.base import Representation
from topos.graphs.mdg.object import LadybugSchemaMismatchError, ModuleDependencyGraph

from .cache import dep_graph_for

_last_dep_graph_error: str | None = None


def last_dep_graph_error() -> str | None:
    """Return the most recent MDG load failure message, if any."""
    return _last_dep_graph_error


def clear_dep_graph_error() -> None:
    """Reset the stored MDG load failure message (primarily for tests)."""
    global _last_dep_graph_error
    _last_dep_graph_error = None


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


def _check_override_warning(
    override: str | Path, project_root: Path
) -> list[str] | None:
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
    return None


def _is_schema_mismatch(message: str | None) -> bool:
    """Whether a dep-graph load error is a storage/schema version mismatch."""
    if not message:
        return False
    return any(
        term in message.lower()
        for term in ("storage version", "different version", "ladybug")
    )


def _dep_graph_load_warning(gitnexus_dir: Path, dep_graph_loaded: bool) -> list[str]:
    if dep_graph_loaded:
        return []
    load_error = last_dep_graph_error()
    is_schema_mismatch = _is_schema_mismatch(load_error)
    if is_schema_mismatch:
        return [
            f"COMPOSABLE not scored — LadybugDB storage version mismatch: {load_error}"
        ]
    return [
        "COMPOSABLE not scored — .gitnexus exists but the dependency graph "
        "could not be loaded; re-run 'topos depgraph generate' and ensure "
        "GitNexus dependencies are installed."
    ]


def gitnexus_warnings(
    override: str | Path | None,
    project_root: Path,
    gitnexus_dir: Path | None,
    *,
    dep_graph_loaded: bool,
) -> list[str]:
    """Explain why COMPOSABLE is unavailable or risky."""
    project_root = project_root.resolve()
    if override:
        warn = _check_override_warning(override, project_root)
        if warn:
            return warn
    elif gitnexus_dir is None:
        return [
            "COMPOSABLE not scored — no .gitnexus directory found; run "
            "'topos depgraph generate' to score this generator."
        ]

    warnings: list[str] = []
    if gitnexus_dir is not None:
        warnings.extend(_dep_graph_load_warning(gitnexus_dir, dep_graph_loaded))
        stale = _stale_gitnexus_warning(project_root, gitnexus_dir)
        if stale:
            warnings.append(stale)
    return warnings


# Stable prefix shared by the producer (this module) and the agent-contract
# consumer (``formatting.build_agent_contract``) so staleness is matched on a
# single marker instead of an ad-hoc substring search over warning prose.
STALE_GITNEXUS_MARKER = "gitnexus index may be stale"


def _stale_gitnexus_warning(project_root: Path, gitnexus_dir: Path) -> str | None:
    graph_mtime = _gitnexus_mtime(gitnexus_dir)
    if graph_mtime <= 0:
        return None
    head_mtime = _git_head_mtime(project_root)
    if head_mtime is None:
        return None
    if graph_mtime < head_mtime:
        return (
            f"{STALE_GITNEXUS_MARKER} — .gitnexus is older than the latest "
            "git commit; run 'topos depgraph generate' before trusting COMPOSABLE."
        )
    return None


def _resolve_ref_mtime(git_dir: Path, ref_line: str) -> float | None:
    ref_path = git_dir / ref_line.removeprefix("ref: ").strip()
    try:
        return ref_path.stat().st_mtime
    except OSError:
        return None


def _git_head_mtime(project_root: Path) -> float | None:
    git_dir = project_root / ".git"
    head = git_dir / "HEAD"
    try:
        head_text = head.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if head_text.startswith("ref: "):
        return _resolve_ref_mtime(git_dir, head_text)
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


@dataclass(frozen=True)
class DepgraphStatus:
    """Structured ``.gitnexus`` state for the depgraph status MCP tool."""

    state: str  # missing | present | stale | load_error | schema_mismatch
    gitnexus_dir: str | None
    gitnexus_mtime: float | None
    git_head_mtime: float | None
    detail: str | None = None


def depgraph_status(
    override: str | Path | None, project_root: Path, target_file: str
) -> DepgraphStatus:
    """Report ``.gitnexus`` availability/freshness without shelling out.

    Loading is wrapped so even a hard DB error (e.g. a locked LadybugDB)
    becomes a structured ``load_error`` rather than an exception.
    """
    project_root = project_root.resolve()
    if override:
        warn = _check_override_warning(override, project_root)
        if warn:
            return DepgraphStatus("missing", None, None, None, detail=warn[0])

    gitnexus_dir = resolve_gitnexus_dir(override, project_root)
    if gitnexus_dir is None:
        return DepgraphStatus(
            "missing",
            None,
            None,
            None,
            detail="No .gitnexus directory found; run topos_generate_depgraph.",
        )

    graph_mtime = _gitnexus_mtime(gitnexus_dir)
    head_mtime = _git_head_mtime(project_root)
    dir_str = str(gitnexus_dir)

    try:
        clear_dep_graph_error()
        dep_graph_for(gitnexus_dir, target_file)
    except Exception as exc:  # noqa: BLE001 — surface any load failure as state
        msg = str(exc)
        state = "schema_mismatch" if _is_schema_mismatch(msg) else "load_error"
        return DepgraphStatus(state, dir_str, graph_mtime, head_mtime, detail=msg)

    stale = head_mtime is not None and graph_mtime > 0.0 and graph_mtime < head_mtime
    return DepgraphStatus(
        "stale" if stale else "present",
        dir_str,
        graph_mtime,
        head_mtime,
        detail=(
            ".gitnexus is older than the latest git commit; regenerate before "
            "trusting COMPOSABLE. (Freshness is mtime-based, not content-hashed.)"
            if stale
            else None
        ),
    )


def _handle_dep_graph_error(exc: Exception) -> ModuleDependencyGraph | None:
    global _last_dep_graph_error
    if isinstance(exc, LadybugSchemaMismatchError):
        _last_dep_graph_error = str(exc)
        return None
    if isinstance(exc, RuntimeError):
        msg = str(exc).lower()
        if "different version" in msg or "storage version" in msg:
            _last_dep_graph_error = str(exc)
            return None
        raise exc
    if isinstance(exc, (FileNotFoundError, ImportError, OSError)):
        _last_dep_graph_error = str(exc)
        return None
    raise exc


def load_dep_graph(
    gitnexus_dir: Path | None, target_file: str
) -> ModuleDependencyGraph | None:
    """Load the cached dep graph for a file, or None if not available."""
    global _last_dep_graph_error
    if gitnexus_dir is None:
        _last_dep_graph_error = None
        return None
    try:
        graph = dep_graph_for(gitnexus_dir, target_file)
        _last_dep_graph_error = None
        return graph
    except Exception as exc:
        return _handle_dep_graph_error(exc)


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


def detect_language(path: Path) -> str:
    """Map a file suffix to a tree-sitter language, defaulting to ``python``."""
    from topos.graphs.ast.dispatch import LANGUAGE_FILE_SUFFIXES

    for lang, suffixes in LANGUAGE_FILE_SUFFIXES.items():
        if path.suffix in suffixes:
            return lang
    return "python"


def classify_file(
    path: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
) -> tuple[ClassificationResult, ModuleDependencyGraph | None]:
    """Classify a file, attaching every available representation.

    Returns ``(result, dep_graph)`` so callers can cache the dep graph
    for subsequent proposed-code evaluations.
    """
    language = detect_language(path)
    morphism = ProgramMorphism.from_file(path, language=language)
    dep_graph = load_dep_graph(gitnexus_dir, str(path))
    result = classify_morphism(morphism, priority, dep_graph)
    return result, dep_graph
