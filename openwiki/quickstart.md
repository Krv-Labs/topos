---
type: Repository Guide
title: Topos code wiki quickstart
description: Entry point for maintaining Topos, a Rust structural code-quality evaluator and MCP agent harness with four-pillar analysis, registration, integration, and release routes.
resource: /README.md
tags: [topos, code-quality, static-analysis, agents, rust]
openwiki:
  roles: [repository, workflow]
  change_kinds: [cli, mcp, analysis, integration, release]
  source_paths: [Cargo.toml, topos/cli/src/main.rs, topos/engine/src/lib.rs, topos/mcp/src/server.rs]
  test_paths: [topos/cli/tests/install_e2e.rs]
  validation_commands: [cargo test --workspace]
---

# Topos code wiki

Topos is a Rust CLI and stdio MCP server for structural code quality. It independently evaluates **SIMPLE**, **COMPOSABLE**, **SECURE**, and **NAVIGABLE**, then combines the satisfied generators into a 16-element lattice: SLOP passes none, BRONZE one, SILVER two, GOLD three, and PLATINUM/IDEAL all four. `topos/engine` owns parsing, graphs, and policy evaluation; `topos/cli` is the human interface and harness registrar; `topos/mcp` is the agent-facing interface.

This wiki is an engineering map, not a replacement for product documentation in `docs/source/`. Start with a task route, then follow its source and test anchors. The [quality model](domain/quality-model.md) defines outcomes, while the [architecture overview](architecture/overview.md) follows source to a verdict.

## What Topos delivers

- `topos evaluate` scores files or directories, discovers supported languages by default, and can emit JSON; `--language` narrows discovery.
- `topos mcp` launches the Rust MCP server over stdio. It combines ten tool routers, static `topos://docs/*` and `topos://build` resources, and the `topos_refactor_until_ideal` prompt.
- `topos install`, `topos uninstall`, and `topos status` manage only Topos-owned MCP registrations in eight agent harnesses; see [agent-harness registration](workflows/harness-registration.md).
- `topos depgraph generate` prepares GitNexus state for cross-module COMPOSABLE evaluation.
- `topos coverage`, `topos compare`, and `topos graphify` are structural analysis surfaces outside the quality lattice.

## Practical starting points

```bash
# Register the installed binary with detected agent harnesses
topos install

# Basic recursive evaluation across discovered languages
topos evaluate src/ -r

# Enable inter-module COMPOSABLE analysis
npm install -g gitnexus@1.6.8
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus

# Workspace checks used by CI
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Topos supports Python, Rust, JavaScript, TypeScript, C++, and Go. For commands, MCP, and GitNexus behavior, use [agent and CLI workflows](workflows/agent-and-cli.md).

## Task routing

| Change area or user intent | Relevant wiki page | Exact source entry points | Important symbols or types | Focused tests | Minimal validation command |
| --- | --- | --- | --- | --- | --- |
| Change a pillar, threshold, security acknowledgement, role exception, or priority | [Quality model](domain/quality-model.md) | `topos/engine/src/evaluation/`, `topos/engine/src/core/characteristic_morphism.rs` | `ClassificationResult`, `Generator`, `evaluate_gates` | adjacent engine unit tests | `cargo test -p topos-engine <filter>` |
| Change NAVIGABLE nesting measurement or its gate | [Quality model](domain/quality-model.md#navigable) | `topos/engine/src/functors/probes/ast/divergence.rs`, `topos/engine/src/evaluation/policies/navigable.rs` | `calculate_max_function_divergence`, `score_navigable` | `nesting_costs_where_sequential_branching_does_not` and policy tests | `cargo test -p topos-engine navigable` |
| Change parsing, UAST mapping, CFG, PDG, CPG, or complexity | [Architecture overview](architecture/overview.md) | `topos/engine/src/graphs/`, `topos/engine/src/functors/probes/` | `node_key`, `NavigableRepresentation`, CFG builder | `graphs/cfg/edge_contracts.rs` and adjacent module tests | `cargo test -p topos-engine <filter>` |
| Change CLI evaluation, command, MCP tool, resource, or agent loop | [Agent and CLI workflows](workflows/agent-and-cli.md) | `topos/cli/src/main.rs`, `topos/cli/src/commands/`, `topos/mcp/src/{server.rs,tools/}` | `Command`, `ToposServer`, `ToolRouter` | CLI/MCP crate tests | `cargo test -p topos` or `cargo test -p topos-mcp` |
| Add or change agent-harness registration | [Agent-harness registration](workflows/harness-registration.md) | `topos/cli/src/commands/install/` | `HARNESSES`, `HarnessSpec`, `Artifact`, `State` | `topos/cli/tests/install_e2e.rs` | `cargo test -p topos --test install_e2e` |
| Change GitNexus, Sighthound, Docker, VS Code, or MCP filesystem boundary | [Integrations and distribution](integrations/distribution.md) | `topos/engine/src/{adapters/gitnexus.rs,graphs/mdg/}`, `topos/mcp/src/security.rs` | `resolve_project_path`, `resolve_within_root`, `ModuleDependencyGraph` | integration-specific crate tests | `cargo test -p topos-mcp` or matching focused engine test |
| Change CI, version metadata, installer, wheel, or release automation | [Testing and release operations](operations/testing-and-release.md) | `.github/workflows/`, `scripts/check_versions.py`, `install.sh` | `check_versions.py --tag` | `tests/packaging/test_install_sh_preflight.py` | `python3 scripts/check_versions.py` |
| Find ownership not listed above | [Source map](source-map.md) | `topos/`, `extensions/vscode/`, `.github/workflows/` | package boundaries | linked page | use the linked page’s focused command |

## Runtime constraints to retain

The engine normalizes six languages into UAST, then derives CFG, PDG, and CPG views. UAST clone/drop/equality and CFG construction are stack-safe. CFG edge contracts lock selected branch/loop, match/switch-return, and try-return layouts across the language registry; preserve those tests when changing traversal.

SIMPLE’s per-function complexity and NAVIGABLE’s worst-function Semantic Compositional Divergence are both UAST-based. NAVIGABLE measures block nesting rather than branch count: sequential branches can be flat, while deep nesting increases the reader’s active structural state. See [the quality model](domain/quality-model.md#navigable) before changing either AST mapper or gate.

## Repository boundaries

- Source and tests are authoritative. Product documentation is in `docs/source/`; this wiki supplies engineering rationale and maintenance routes.
- `Cargo.toml` owns the workspace version. `scripts/check_versions.py` checks distribution metadata alignment and `--tag` checks release tag alignment.
- `openwiki/INSTRUCTIONS.md` is a user-authored scope brief; routine wiki maintenance does not rewrite it.
- This checkout has a shallow, grafted `HEAD`; the prior `gitHead` in update metadata is unavailable locally. This update is grounded in current source and the release snapshot rather than an unavailable revision range.

## Backlog

- **Formal lattice ordering** — `topos/engine/src/core/omega.rs`: verify the explanatory mathematical ordering against implemented helpers before expanding theory documentation.
- **Parser backend semantics** — `topos/engine/src/graphs/ast/dispatch.rs`: document an additional parser-backend contract only after it is implemented and tested.
