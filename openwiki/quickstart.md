---
type: Repository Guide
title: Topos code wiki quickstart
description: Entry point for maintaining Topos, a Rust structural code-quality evaluator and MCP agent harness with analysis, registration, integration, and release routes.
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

Topos is a self-contained Rust CLI and stdio MCP server for structural code quality. It measures **SIMPLE**, **COMPOSABLE**, and **SECURE** independently, then maps their combination to a medal from SLOP to GOLD. `topos/engine` owns parsing, graphs, and policy evaluation; `topos/cli` exposes them to people and can register the MCP server in supported agent harnesses; `topos/mcp` exposes the same engine to agents.

This wiki is an engineering map, not a replacement for the product documentation in `docs/source/`. Start with the task route below, then follow its source and test anchors.

## What Topos delivers

- `topos evaluate` scores source files and can emit terminal or JSON results. It discovers all supported languages by default; `--language` narrows the run.
- `topos mcp` launches the in-process Rust MCP server over stdio.
- `topos install`, `topos uninstall`, and `topos status` manage only Topos-owned MCP registrations in eight agent harnesses; see [agent-harness registration](workflows/harness-registration.md).
- `topos depgraph generate` prepares GitNexus state for cross-file COMPOSABLE evaluation.
- `topos graphify` and MCP refactor tools provide advisory structural findings that do **not** alter the three-pillar medal.

The [quality model](domain/quality-model.md) explains verdict meaning. The [architecture overview](architecture/overview.md) follows source from tree-sitter parsing through program graphs to a lattice result.

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

# Run the Rust workspace checks used by CI
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Topos supports Python, Rust, JavaScript, TypeScript, C++, and Go. For command, MCP, and GitNexus behavior, read [agent and CLI workflows](workflows/agent-and-cli.md).

## Task routing

| Change area or user intent | Relevant wiki page | Exact source entry points | Important symbols or types | Focused tests | Minimal validation command |
| --- | --- | --- | --- | --- | --- |
| Change a pillar, threshold, security acknowledgement, or role exception | [Quality model](domain/quality-model.md) | `topos/engine/src/evaluation/`, `topos/engine/src/core/characteristic_morphism.rs` | policy translators, `ClassificationResult`, suppression overlay | adjacent engine unit tests | `cargo test -p topos-engine <filter>` |
| Change parsing, UAST mapping, CFG, PDG, CPG, or complexity | [Architecture overview](architecture/overview.md) | `topos/engine/src/graphs/`, `topos/engine/src/functors/probes/` | `node_key`, CFG builder, `ast.max_function_complexity` | `graphs/cfg/edge_contracts.rs` and adjacent module tests | `cargo test -p topos-engine <filter>` |
| Change CLI evaluation, a command, MCP tool, resource, or agent loop | [Agent and CLI workflows](workflows/agent-and-cli.md) | `topos/cli/src/main.rs`, `topos/cli/src/commands/`, `topos/mcp/src/{server.rs,tools/}` | `Command`, `ToposServer`, `ToolRouter` | CLI/MCP crate tests | `cargo test -p topos` or `cargo test -p topos-mcp` |
| Add or change agent-harness registration | [Agent-harness registration](workflows/harness-registration.md) | `topos/cli/src/commands/install/` | `HARNESSES`, `HarnessSpec`, `Artifact`, `State` | `topos/cli/tests/install_e2e.rs` | `cargo test -p topos --test install_e2e` |
| Change GitNexus, Sighthound, Docker, VS Code, or MCP packaging | [Integrations and distribution](integrations/distribution.md) | `topos/engine/src/{adapters/gitnexus.rs,graphs/mdg/}`, `topos/mcp/src/security.rs` | `resolve_within_root`, `ModuleDependencyGraph` | integration-specific crate tests; CI fixture | `cargo test -p topos-mcp` or matching focused engine test |
| Change CI, version metadata, installer, wheel, or release automation | [Testing and release operations](operations/testing-and-release.md) | `.github/workflows/`, `scripts/check_versions.py`, `install.sh` | `check_versions.py --tag` | `tests/packaging/test_install_sh_preflight.py` | `python3 scripts/check_versions.py` |
| Find ownership not listed above | [Source map](source-map.md) | `topos/`, `extensions/vscode/`, `.github/workflows/` | package boundaries | linked page | use the linked page’s focused command |

## Current engineering constraints

The engine normalizes six languages into UAST, then derives CFG, PDG, and CPG views. Deep-tree handling is stack-safe for UAST clone/drop/equality and CFG construction. CFG edge contracts lock selected branch/loop, match/switch-return, and try-return layouts across the language registry; preserve those tests when changing traversal.

SIMPLE’s per-function complexity is cross-language UAST based. It counts decision forms including short-circuit booleans, ternaries, Python comprehension clauses, try handlers, and match/switch arms, so unchanged thresholds apply to a fuller structural model. See [the architecture boundary](architecture/overview.md#simple-complexity-boundary) before changing mappers or gates.

## Repository boundaries

- Source and tests are authoritative. Product documentation lives in `docs/source/`; this wiki explains engineering rationale and maintenance routes.
- `Cargo.toml` owns the workspace version. `scripts/check_versions.py` checks distribution metadata alignment and, when passed `--tag`, tag alignment.
- `openwiki/INSTRUCTIONS.md` is a user-authored scope brief; do not rewrite it during normal wiki maintenance.
- The untracked `.github/workflows/openwiki-update.yml` is operational metadata, not Topos runtime code.

## Backlog

- **Formal lattice ordering** — `topos/engine/src/core/omega.rs`: verify the explanatory mathematical ordering against implemented helpers before expanding theory documentation.
- **Parser backend semantics** — `topos/engine/src/graphs/ast/dispatch.rs`: document any additional parser-backend contract only after it is implemented and tested.
