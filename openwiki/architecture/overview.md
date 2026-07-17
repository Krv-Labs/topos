---
type: Architecture Overview
title: Hybrid analysis and evaluation architecture
description: Explains how Topos turns supported-language source into normalized program graphs, policy decisions, and lattice verdicts using Python orchestration and Rust kernels.
resource: /topos/core/morphism.py
tags: [architecture, python, rust, program-graphs, evaluation]
---

# Hybrid analysis and evaluation architecture

Topos treats a source file as a `ProgramMorphism`: source plus parse result and derived structural representations. It is not a monolithic compiler. Python coordinates language parsing, UAST mapping, graph assembly, policies, CLI, and MCP; the Maturin-built `topos.topos_functors` extension provides selected Rust implementations for CFG algorithms, entropy, edit distance, curvature, and cycle analysis.

The [quality model](../domain/quality-model.md) assigns product meaning to the outputs of this pipeline, while [agent and CLI workflows](../workflows/agent-and-cli.md) expose it to humans and agents.

## Evaluation flow

1. **Parse source.** `ProgramMorphism` creates a `ProgramObject` via `topos.graphs.ast.dispatch.parse_source` (`topos/core/morphism.py`). Tree-sitter provides the common parsing/UAST route for Python, Rust, JavaScript, TypeScript, C++, and Go. The default backend is named `hybrid`; native parsing is currently substantive only for Python, so avoid assuming cross-language dual-parser behavior.
2. **Build representations.** A morphism lazily caches CFG, PDG, CPG, and abstractness representations from its UAST. These representations are different views of the same file, not independent source parsers.
3. **Attach cross-file information when available.** A `ModuleDependencyGraph` reads GitNexus output to represent imports, calls, inheritance, and other relationships across files. This is why [COMPOSABLE depends on the integration](../integrations/distribution.md#gitnexus-for-composable).
4. **Measure and score.** `CharacteristicMorphism.classify_detailed` always adds AST metrics, groups supplied representations by dimension, invokes dimension scorers, and records raw metrics, interpretations, scores, and achieved generators.
5. **Aggregate project results.** `combine_dimensions` takes the minimum score per dimension across evaluated files. A parse failure injects zero SIMPLE score, so project-level success is intentionally pessimistic.

## Representation boundaries

| Representation | Scope and primary use | Key anchors |
| --- | --- | --- |
| AST/UAST | Per-file syntax and normalized language shape; entropy and general graph input | `topos/graphs/ast/`, `topos/graphs/uast/` |
| CFG | Intra-file control flow; cyclomatic/essential complexity, nesting, longest acyclic path | `topos/graphs/cfg/` |
| PDG | Intra-procedural control and data dependence; feeds CPG diagnostics | `topos/graphs/pdg/` |
| CPG | AST/CFG/DDG/CDG fusion; SECURE metrics | `topos/graphs/cpg/` |
| MDG | Inter-module GitNexus graph; coupling, instability, dependency depth | `topos/graphs/mdg/object.py` |
| Process graph | GitNexus process transitions; advisory curvature hotspots only | `topos/graphs/process/` |

The CPG dispatches to Sighthound when present and otherwise runs local probes, connecting its SECURE metrics directly to [external security integration behavior](../integrations/distribution.md#sighthound-for-secure). The process graph and refactor probes deliberately remain outside medal scoring; see [advisory workflow guidance](../workflows/agent-and-cli.md#advisory-refactoring).

## UAST contract and dependency-graph limits

The UAST is a normalized representation used for cross-language structural analysis; it is not a claim that Topos retains every construct from every native parser. `topos/graphs/uast/mapper_python.py` maps common declaration, statement, expression, identifier, literal, and file kinds while preserving source positions through the shared mapper. In particular, its mapping tables do **not** currently include Python `import_statement` or `import_from_statement`, so those nodes become `Unknown`. Other language mappers should be checked individually rather than assumed to implement a proposed universal schema.

This boundary matters to COMPOSABLE assessment. GitNexus owns cross-file module and symbol resolution, and Topos consumes its `.gitnexus` output as an MDG. Although an edited source variant can be parsed, Topos cannot correctly reconstruct its outgoing MDG edges from the UAST: import extraction, first-party module resolution, and a supported per-file MDG edge-replacement API are absent. Furthermore, instability is import-driven but `fan_out` is based on resolved `CALLS` edges. The assessment path therefore reports the static-graph approximation and freshness warnings instead of silently claiming an updated COMPOSABLE verdict.

Do not add an incremental MDG patch as a small scoring change. It needs an import/outbound-reference representation, resolution against MDG file nodes (including packages and third-party imports), safely maintained incoming/outgoing indexes, and parity tests against a fresh `gitnexus analyze`. First measure whether stale-versus-regenerated topology changes COMPOSABLE verdicts often enough to justify that infrastructure; a fast full regeneration is the correctness-preserving alternative. This limitation is surfaced to agents through the [CLI and MCP workflows](../workflows/agent-and-cli.md#mcp-agent-loop) and derives from the external [GitNexus integration](../integrations/distribution.md#gitnexus-for-composable).

## Native-code boundary

`pyproject.toml` configures Maturin with `Cargo.toml` as the version source and publishes the extension as `topos.topos_functors`. `src/lib.rs` exports native types/functions rather than a complete application runtime.

Rust is used where structural operations are expensive or algorithmically sensitive:

- CFG complexity and longest acyclic path (`src/cfg.rs`)
- cycle-basis extraction (`src/ph.rs`)
- balanced and directed Forman curvature (`src/frc.rs`)
- compression-based AST entropy (`src/probes_ast.rs`)
- sequence edit distance (`src/profunctors.rs`)

Keep Python-facing models and Rust exports in sync. Changes crossing this boundary should include targeted Python tests and, where applicable, `tests/parity/`; [testing operations](../operations/testing-and-release.md) describe the full gate.

## Design constraints to preserve

- CFG longest-path calculation assumes loopback and continue edges are removed before DAG dynamic programming. A previous exponential path enumeration was replaced for this reason (`CHANGELOG.md`, 0.3.8).
- The classifier only automatically supplies AST metrics; callers must supply CFG/CPG/MDG/abstractness representations appropriate to the desired dimensions. Trace the invoking CLI or MCP path before diagnosing a missing pillar.
- MDG loading gracefully degrades for missing, stale, incompatible, or branch-mismatched GitNexus state. It can retry a shadow-page replay with read-write access, so do not describe dependency-graph evaluation as unconditionally read-only.
- Source spans are used for diagnostics and CPG text recovery. Preserve source/parse revision alignment when extending graph models.

## Change navigation

- Parsing/language support: `topos/graphs/ast/languages.py`, AST providers, and `topos/graphs/uast/mapper_*.py`.
- Per-file graph semantics: `topos/graphs/{cfg,pdg,cpg}/` and matching `tests/graphs/`.
- Policy coupling and lattice assembly: `topos/evaluation/characteristic_morphism.py`, then [quality model](../domain/quality-model.md).
- Native algorithm changes: `src/` plus Python probe adapters under `topos/functors/`.
