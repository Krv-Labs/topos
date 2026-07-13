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

import contextlib
import json
import logging
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
from topos.utils.gitnexus import GITNEXUS_FINGERPRINT_FILE, source_fingerprint

from .cache import dep_graph_for

# Debug-only: which freshness method decided a verdict, and the calibration
# internals in _newer_source_file. Never rises above DEBUG — this exists so a
# suspicious freshness verdict is diagnosable from logs instead of only from
# reading this module's source, not to surface anything to MCP callers.
_logger = logging.getLogger(__name__)

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


# Stable prefixes shared by the producer (this module) and the agent-contract
# consumer (``formatting.build_agent_contract``) so an invalid/denied override
# is matched on a single marker instead of ad-hoc substring searches. An invalid
# override is distinct from a missing graph: regenerating in the project root
# won't fix a bad path, so the contract must tell the agent to fix the path.
INVALID_GITNEXUS_MARKERS = ("gitnexus_dir rejected", "gitnexus_dir unavailable")


def _check_override_warning(
    override: str | Path, project_root: Path
) -> list[str] | None:
    override_path = Path(override).expanduser().resolve()
    try:
        override_path.relative_to(project_root)
    except ValueError:
        return [
            f"{INVALID_GITNEXUS_MARKERS[0]} — override must be inside "
            f"TOPOS_MCP_FILE_ROOT. Got: {override_path}"
        ]
    if not override_path.exists():
        return [
            f"{INVALID_GITNEXUS_MARKERS[1]} — override path does not exist. "
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


@dataclass(frozen=True)
class GraphFingerprint:
    """Topos-owned generation marker: what and when the graph was built from.

    v1 markers carry only ``head_sha``; ``generated_at`` is the v2 field that
    enables working-tree freshness (in-place edits never move HEAD).
    """

    head_sha: str | None
    generated_at: float | None
    finished_at: float | None = None
    source_hash: str | None = None
    source_file_count: int | None = None


def _read_graph_fingerprint(gitnexus_dir: Path) -> GraphFingerprint | None:
    """The graph's generation marker, tolerant of v1 payloads (or None)."""
    try:
        raw = (gitnexus_dir / GITNEXUS_FINGERPRINT_FILE).read_text(encoding="utf-8")
        payload = json.loads(raw)
        sha = payload.get("head_sha")
        generated_at = payload.get("generated_at")
        finished_at = payload.get("finished_at")
        source_hash = payload.get("source_hash")
        source_file_count = payload.get("source_file_count")
    except (OSError, ValueError, AttributeError):
        return None
    return GraphFingerprint(
        head_sha=sha if isinstance(sha, str) and sha else None,
        generated_at=(
            float(generated_at) if isinstance(generated_at, (int, float)) else None
        ),
        finished_at=(
            float(finished_at) if isinstance(finished_at, (int, float)) else None
        ),
        source_hash=source_hash if isinstance(source_hash, str) else None,
        source_file_count=(
            int(source_file_count) if isinstance(source_file_count, int) else None
        ),
    )


# Freshness stat-walk ceiling: beyond this many source files the mtime pass
# returns "fresh" rather than making every evaluate call pay for a pathological
# monorepo walk. The SHA anchor still catches commit-level drift there.
_FRESHNESS_WALK_CAP = 20_000

# Tolerance for the mtime-vs-generated_at comparison: generated_at is a
# sub-second time.time() stamp, but filesystem mtimes can be truncated to
# whole seconds (older HFS+, some network/FUSE mounts), and clocks between
# the process and the filesystem can drift slightly. Comparing with a small
# grace window trades a rare unnecessary regenerate for not silently missing
# a real in-place edit made moments after generation.
_MTIME_SKEW_TOLERANCE_S = 2.0

# Sanity bound on the measured (finished_at - generated_at) duration used to
# calibrate filesystem-clock drift. finished_at is always recorded after
# generated_at within the same _write_fingerprint call, so a negative
# duration means the system clock was adjusted backward mid-generation; an
# implausibly large one means a forward jump or a corrupted field. Either way
# the single-sample drift estimate can't be trusted — fall back to the flat
# tolerance rather than calibrate against a bad sample. `gitnexus analyze`
# can legitimately run for minutes on a large repo (see
# DEFAULT_ANALYZE_TIMEOUT_S), so this ceiling is deliberately generous.
_MAX_TRUSTED_GENERATION_DURATION_S = 3600.0


def _newer_source_file(
    project_root: Path,
    generated_at: float,
    finished_at: float | None = None,
    fingerprint_mtime: float | None = None,
) -> Path | None:
    """First source file (or directory) modified after *generated_at*, or None.

    Stat-only walk over the same discovery pruning the evaluators use (skips
    .git/.gitnexus/venvs/node_modules/build dirs). Deliberately avoids the
    git-aware ignore checker — it shells out per path, which is unaffordable
    on every freshness probe.

    Directories are stat'd too: deleting or renaming a file leaves no file of
    its own to catch, but it does bump its parent directory's mtime, which a
    file-only walk would silently miss.
    """
    from topos.graphs.ast.languages import LANGUAGE_FILE_SUFFIXES
    from topos.utils.discovery import iter_source_files

    suffixes = tuple(
        {suffix for group in LANGUAGE_FILE_SUFFIXES.values() for suffix in group}
    )
    # Default to the flat-tolerance threshold; only override it with the
    # clock-drift-calibrated value when finished_at/fingerprint_mtime are
    # available *and* the duration they imply is plausible.
    threshold = generated_at - _MTIME_SKEW_TOLERANCE_S
    if finished_at is not None and fingerprint_mtime is not None:
        duration = finished_at - generated_at
        if 0 <= duration <= _MAX_TRUSTED_GENERATION_DURATION_S:
            if fingerprint_mtime % 1.0 == 0.0:
                drift = int(finished_at) - fingerprint_mtime
                threshold = int(generated_at) - drift
            else:
                drift = finished_at - fingerprint_mtime
                threshold = generated_at - drift
            _logger.debug(
                "depgraph freshness: mtime-calibrated threshold=%.3f (duration=%.3fs)",
                threshold,
                duration,
            )
        else:
            _logger.debug(
                "depgraph freshness: distrusting drift calibration "
                "(duration=%.3fs outside [0, %.0f]s); using flat tolerance",
                duration,
                _MAX_TRUSTED_GENERATION_DURATION_S,
            )
    seen = 0
    for path in iter_source_files(project_root, suffixes=suffixes, include_dirs=True):
        is_file = path.suffix in suffixes
        if is_file:
            seen += 1
            if seen > _FRESHNESS_WALK_CAP:
                return None
        try:
            if path.stat().st_mtime > threshold:
                return path
        except OSError:
            continue
    return None


def _graph_freshness(project_root: Path, gitnexus_dir: Path) -> tuple[bool, str | None]:
    """Whether the dependency graph is stale w.r.t. the working tree.

    Two anchors, both recorded at generation time:

    1. Commit SHA — catches checkouts and new commits; a regenerate reliably
       clears it.
    2. ``generated_at`` — the graph's content reflects the working tree at
       generation, so any source file modified afterwards (an in-place edit
       that never moves HEAD) also invalidates COMPOSABLE.

    Graphs from before the fingerprint marker fall back to comparing the
    graph DB mtime to the latest commit's mtime. Returns ``(is_stale, detail)``.
    """
    fingerprint = _read_graph_fingerprint(gitnexus_dir)
    if fingerprint is not None and fingerprint.source_hash is not None:
        current = source_fingerprint(project_root)
        is_stale = current.content_hash != fingerprint.source_hash
        _logger.debug("depgraph freshness: method=content_hash stale=%s", is_stale)
        if is_stale:
            return True, (
                f"{STALE_GITNEXUS_MARKER} — source tree content changed since "
                "the dependency graph was generated; run 'topos depgraph "
                "generate' before trusting COMPOSABLE."
            )
        return False, None

    graph_sha = fingerprint.head_sha if fingerprint else None
    head_sha = _git_head_sha(project_root)
    sha_anchored = graph_sha is not None and head_sha is not None
    if sha_anchored and graph_sha != head_sha:
        _logger.debug("depgraph freshness: method=sha_anchor stale=True")
        return True, (
            f"{STALE_GITNEXUS_MARKER} — graph was built from commit "
            f"{graph_sha[:7]} but HEAD is {head_sha[:7]}; run "
            "'topos depgraph generate' before trusting COMPOSABLE."
        )

    if fingerprint is not None and fingerprint.generated_at is not None:
        fingerprint_file = gitnexus_dir / GITNEXUS_FINGERPRINT_FILE
        fingerprint_mtime = None
        if fingerprint_file.exists():
            with contextlib.suppress(OSError):
                fingerprint_mtime = fingerprint_file.stat().st_mtime
        newer = _newer_source_file(
            project_root,
            generated_at=fingerprint.generated_at,
            finished_at=fingerprint.finished_at,
            fingerprint_mtime=fingerprint_mtime,
        )
        _logger.debug(
            "depgraph freshness: method=mtime_calibrated stale=%s", newer is not None
        )
        if newer is not None:
            try:
                rel = newer.relative_to(project_root)
            except ValueError:
                rel = newer
            return True, (
                f"{STALE_GITNEXUS_MARKER} — {rel} was modified after the "
                "dependency graph was generated; run 'topos depgraph generate' "
                "before trusting COMPOSABLE."
            )
        return False, None

    if sha_anchored:
        # v1 fingerprint (SHA only, matching HEAD): no working-tree signal
        # available — preserve the legacy PRESENT verdict.
        _logger.debug("depgraph freshness: method=sha_only_no_signal stale=False")
        return False, None

    # Legacy fallback: no fingerprint marker (or HEAD unresolvable) — compare the
    # graph DB mtime to the latest commit's mtime.
    graph_mtime = _gitnexus_mtime(gitnexus_dir)
    head_mtime = _git_head_mtime(project_root)
    if graph_mtime <= 0 or head_mtime is None:
        _logger.debug("depgraph freshness: method=legacy_dir_mtime unavailable")
        return False, None
    is_stale = graph_mtime < head_mtime
    _logger.debug("depgraph freshness: method=legacy_dir_mtime stale=%s", is_stale)
    if is_stale:
        return True, (
            f"{STALE_GITNEXUS_MARKER} — .gitnexus is older than the latest "
            "git commit; run 'topos depgraph generate' before trusting COMPOSABLE."
        )
    return False, None


def _stale_gitnexus_warning(project_root: Path, gitnexus_dir: Path) -> str | None:
    _stale, detail = _graph_freshness(project_root, gitnexus_dir)
    return detail


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


def _packed_ref_sha(git_dir: Path, ref: str) -> str | None:
    """Resolve ``ref`` from ``.git/packed-refs`` (loose ref absent)."""
    try:
        lines = (git_dir / "packed-refs").read_text(encoding="utf-8").splitlines()
    except OSError:
        return None
    for line in lines:
        if line.startswith(("#", "^")):
            continue
        parts = line.split(maxsplit=1)
        if len(parts) == 2 and parts[1].strip() == ref:
            return parts[0].strip()
    return None


def _git_head_sha(project_root: Path) -> str | None:
    """Current HEAD commit SHA, read from ``.git`` without shelling out.

    Mirrors ``_git_head_mtime``: resolves a symbolic HEAD via its loose ref, then
    ``packed-refs``; a detached HEAD stores the SHA directly. Returns ``None`` for
    a non-git dir or an unborn/unresolvable HEAD (freshness then falls back).
    """
    git_dir = project_root / ".git"
    try:
        head_text = (git_dir / "HEAD").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if not head_text.startswith("ref: "):
        return head_text or None  # detached HEAD: contents are the SHA
    ref = head_text.removeprefix("ref: ").strip()
    try:
        return (git_dir / ref).read_text(encoding="utf-8").strip() or None
    except OSError:
        return _packed_ref_sha(git_dir, ref)


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

    state: str  # missing | present | stale | load_error | schema_mismatch | invalid_dir
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
            return DepgraphStatus("invalid_dir", None, None, None, detail=warn[0])

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

    stale, detail = _graph_freshness(project_root, gitnexus_dir)
    return DepgraphStatus(
        "stale" if stale else "present",
        dir_str,
        graph_mtime,
        head_mtime,
        detail=detail,
    )


def _handle_dep_graph_error(exc: Exception) -> ModuleDependencyGraph | None:
    global _last_dep_graph_error
    if isinstance(exc, LadybugSchemaMismatchError):
        _last_dep_graph_error = str(exc)
        return None
    if isinstance(exc, RuntimeError):
        # Every RuntimeError reaching here originates from LadybugDB loading
        # (ModuleDependencyGraph._from_ladybugdb) — e.g. a corrupted WAL or a
        # shadow-page replay that failed even read-write. Degrade COMPOSABLE
        # rather than aborting the whole evaluate/MCP invocation.
        _last_dep_graph_error = str(exc)
        return None
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
    Build the intrinsic representations derived from the UAST: CFG,
    academic PDG, CPG, Abstractness.  These require no external tooling so
    they are always attached.  Missing UAST (parse failure) yields an
    empty list.
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
    abstractness = morphism.build_abstractness()
    if abstractness is not None:
        reps.append(abstractness)
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
    from topos.graphs.ast.languages import LANGUAGE_FILE_SUFFIXES

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
