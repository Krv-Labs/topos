---
type: Source Map
title: Topos maintenance source map
description: Maps Topos maintenance tasks to current Rust workspace source, tests, documentation, and automation locations without duplicating domain behavior.
resource: /topos
tags: [source-map, maintenance, navigation, rust]
openwiki:
  roles: [repository, testing]
  change_kinds: [navigation]
  source_paths: [topos, extensions/vscode, .github/workflows]
---

# Topos maintenance source map

Use this map to start implementation work, then follow the linked concept page for behavior and constraints. It focuses on ownership boundaries rather than every file.

| Area | Primary source | Tests / supporting docs | Read first |
| --- | --- | --- | --- |
| Workspace metadata | `Cargo.toml`, `topos/{engine,cli,mcp}/Cargo.toml` | `scripts/check_versions.py` | [Testing and release operations](operations/testing-and-release.md) |
| Categorical core and lattice | `topos/engine/src/core/` | crate unit tests | [Architecture overview](architecture/overview.md) |
| Parsing, language registry, UAST mapping | `topos/engine/src/graphs/{ast,uast}/` | module unit tests, `docs/decisions/uast-industry-standards.md` | [Architecture overview](architecture/overview.md) |
| CFG, PDG, CPG graphs | `topos/engine/src/graphs/{cfg,pdg,cpg}/`, `functors/probes/cfg/` | inline unit tests; `cfg/edge_contracts.rs` locks cross-language CFG layouts and nesting-depth regressions protect loop handling | [Architecture overview](architecture/overview.md) |
| Metrics, NAVIGABLE divergence, and structural comparisons | `topos/engine/src/functors/` | inline crate tests; `probes/ast/divergence.rs` distinguishes nested from sequential branching | [Quality model](domain/quality-model.md) |
| Evaluation, four-pillar gates, preferences, suppression | `topos/engine/src/evaluation/`, `topos/engine/src/config.rs`, `core/{omega.rs,characteristic_morphism.rs}` | inline crate tests and MCP result behavior | [Quality model](domain/quality-model.md) |
| CLI | `topos/cli/src/{main.rs,commands/}` | CLI crate tests and direct command smoke cases | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| Agent-harness MCP registration | `topos/cli/src/commands/install/` | `topos/cli/tests/install_e2e.rs` drives the real binary and snapshots `$HOME` | [Agent-harness registration](workflows/harness-registration.md) |
| MCP server, schemas, tools, resources | `topos/mcp/src/`; project rollup/ranking: `topos/mcp/src/tools/evaluate.rs` | crate tests; `ranking_lists_evaluate_gates_once_per_row` protects one gate evaluation per ranked row; CI stdio smoke test | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| GitNexus dependency topology | `topos/engine/src/{adapters/gitnexus.rs,graphs/mdg/}` | `.github/workflows/ci.yml` composable fixture | [Integrations and distribution](integrations/distribution.md) |
| Embedded Sighthound handling | `topos/mcp/src/{security.rs,security_findings.rs,sighthound.rs}` | MCP crate tests | [Integrations and distribution](integrations/distribution.md) |
| Graphify / advisory analysis | `topos/cli/src/commands/graphify/`, `topos/mcp/src/tools/{graphify.rs,refactor.rs}` | crate tests | [Agent and CLI workflows](workflows/agent-and-cli.md#advisory-and-non-lattice-analysis) |
| MCP file-root containment | `topos/mcp/src/security.rs` | symlink and missing-path regression tests in the module | [Integrations and distribution](integrations/distribution.md#mcp-file-access-boundary) |
| Container / MCP registry | `Dockerfile`, `.mcp/server.json`, `scripts/check_versions.py` | wheel and release workflow; metadata/tag guard | [Integrations and distribution](integrations/distribution.md) |
| VS Code extension | `extensions/vscode/` | package scripts and release workflow | [Integrations and distribution](integrations/distribution.md) |
| CI, docs, releases | `.github/workflows/`, `install.sh`, `docs/source/` | workflow jobs | [Testing and release operations](operations/testing-and-release.md) |

## Fast triage routes

- **Unexpected medal or missing dimension:** trace CLI/MCP input into `topos_engine::core::characteristic_morphism`, `core::omega::Generator`, and the representation/policy that contributes SIMPLE, COMPOSABLE, SECURE, or NAVIGABLE.
- **CFG/CPG/PDG issue:** start with the Rust graph builder and its adjacent unit tests; preserve UAST node-key consistency when an edge crosses graph families.
- **COMPOSABLE unavailable or stale:** inspect dependency-graph status, then GitNexus path/schema/freshness before changing policy.
- **SECURE mismatch:** distinguish native CPG evidence from MCP embedded-Sighthound normalization.
- **Published artifact mismatch:** run version checks, then inspect Cargo manifests, `.mcp/server.json`, VS Code metadata, Docker, installer, and release workflow.

## Repository layout notes

- `topos/engine` is pure analysis code shared by `topos` and `topos-mcp`; it must not acquire CLI, MCP transport, or Python-binding responsibilities.
- Inline Rust unit tests are the primary regression suite after the Python implementation and its pytest suite were removed.
- `docs/source/` is the Sphinx product documentation source; this wiki provides engineering rationale and maintenance navigation.
- `extensions/vscode/` remains a separately built TypeScript package; Cargo tests cannot verify it.
