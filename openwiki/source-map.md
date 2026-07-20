---
type: Source Map
title: Topos maintenance source map
description: Maps common Topos maintenance tasks to the primary source, test, documentation, and automation locations without duplicating domain behavior.
resource: /topos
tags: [source-map, maintenance, navigation]
---

# Topos maintenance source map

Use this map to start implementation work, then follow the linked concept page for behavior and constraints. It intentionally focuses on ownership boundaries rather than every file.

| Area | Primary source | Tests / supporting docs | Read first |
| --- | --- | --- | --- |
| Package/runtime metadata | `pyproject.toml`, `Cargo.toml`, `topos/_version.py` | `scripts/check_versions.py`, `tests/test_version.py` | [Testing and release operations](operations/testing-and-release.md) |
| Core program abstraction and lattice | `topos/core/` | `tests/core/` | [Architecture overview](architecture/overview.md) |
| Parsing, language registry, UAST mapping | `topos/graphs/ast/`, `topos/graphs/uast/` | `tests/graphs/ast/`, `tests/graphs/uast/`, fixtures | [Architecture overview](architecture/overview.md) |
| CFG, PDG, CPG graphs | `topos/graphs/{cfg,pdg,cpg}/` | `tests/graphs/{cfg,pdg,cpg}/`, `tests/functors/probes/` | [Architecture overview](architecture/overview.md) |
| Rust metric/graph kernels | `src/` | Rust unit tests, `tests/parity/`, Python probe tests | [Architecture overview](architecture/overview.md) |
| Evaluation decisions and suggestions | `topos/evaluation/` | `tests/evaluation/`, `tests/mcp/` | [Quality model](domain/quality-model.md) |
| Security acknowledgement config | `topos/config.py`, `topos/evaluation/suppression.py` | evaluation/CLI/MCP tests | [Quality model](domain/quality-model.md) |
| Click CLI | `topos/cli/main.py`, `topos/cli/commands/`, `topos/cli/evaluation.py` | `tests/cli/`, benchmark tests | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| FastMCP server, schemas, tools, resources | `topos/mcp/` | `tests/mcp/`, `docs/source/agents.rst` | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| GitNexus dependency topology | `topos/graphs/mdg/`, `topos/utils/gitnexus.py` | `tests/utils/test_gitnexus.py`, MDG/MCP tests | [Integrations and distribution](integrations/distribution.md) |
| Sighthound security adapter | `topos/utils/sighthound.py`, `topos/mcp/security_findings.py` | `tests/utils/test_sighthound.py` | [Integrations and distribution](integrations/distribution.md) |
| Advisory refactor features | `topos/cli/commands/refactor.py`, `topos/mcp/tools/refactor.py`, probes under `topos/functors/probes/` | `tests/cli/test_refactor.py`, `tests/mcp/test_refactor.py`, Rust tests in `src/ph.rs` / `src/frc.rs` | [Agent and CLI workflows](workflows/agent-and-cli.md#advisory-refactoring) |
| Container / MCP registry | `Dockerfile`, `.mcp/server.json`, `glama.json` | packaging tests, release workflow | [Integrations and distribution](integrations/distribution.md) |
| VS Code extension | `extensions/vscode/` | extension unit/integration scripts and README | [Integrations and distribution](integrations/distribution.md) |
| CI, docs, release automation | `.github/workflows/`, `scripts/build-binary.sh` | CI workflow itself, `docs/source/` | [Testing and release operations](operations/testing-and-release.md) |

## Repository layout notes

- `topos/functors/` bridges graph representations to probes, metrics, and transformations; it is often the correct adapter layer for native Rust changes rather than `topos/graphs/` itself.
- `tests/fixtures/` contains language and scenario inputs; do not apply production formatting/lint rules indiscriminately there.
- `docs/source/` is the Sphinx product-documentation source. Engineering rationale formerly stored as loose `docs/*.md` notes is maintained in this wiki: see the [architecture overview](architecture/overview.md#uast-contract-and-dependency-graph-limits), [agent and CLI workflows](workflows/agent-and-cli.md), and [testing/release operations](operations/testing-and-release.md#cli-startup-and-frozen-binary-guardrails).
- `extensions/vscode/` is a separately built TypeScript package. A Python-only test run cannot verify its behavior.

## Fast triage routes

- **Unexpected medal / missing dimension:** trace CLI or MCP construction into `run_classify_file` / MCP evaluation, then representations passed to `CharacteristicMorphism` and the corresponding policy.
- **COMPOSABLE unavailable or stale:** inspect `topos_depgraph_status`, GitNexus path/freshness, then MDG loader errors before changing policy.
- **SECURE mismatch:** determine whether Sighthound is on `PATH`; test external JSON normalization and local CPG fallback separately.
- **Published artifact mismatch:** run version checks, then inspect `pyproject.toml`, `.mcp/server.json`, `extensions/vscode/package.json`, Docker/release workflow, and packaging tests.
