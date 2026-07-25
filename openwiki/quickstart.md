---
type: Repository Guide
title: Topos code wiki quickstart
description: Entry point for maintaining Topos, a Rust structural code-quality evaluator for coding agents, including its analysis model, interfaces, integrations, and release checks.
resource: /README.md
tags: [topos, code-quality, static-analysis, agents, rust]
---

# Topos code wiki

Topos is a self-contained Rust CLI and stdio MCP server that evaluates source structure for coding agents. It measures **SIMPLE**, **COMPOSABLE**, and **SECURE** independently, then maps their combination to a medal from SLOP to GOLD. `topos/engine` owns parsing, graph construction, and policy evaluation; `topos/cli` and `topos/mcp` expose that shared engine.

## What Topos delivers

- `topos evaluate` scores source files and can emit terminal or JSON results.
- `topos mcp` launches the in-process Rust MCP server over stdio.
- `topos depgraph generate` prepares GitNexus state for cross-file COMPOSABLE evaluation.
- `topos graphify` and MCP refactor tools provide advisory structural findings that do **not** alter the three-pillar medal.

The [quality model](domain/quality-model.md) explains verdict meaning. The [architecture overview](architecture/overview.md) follows source from tree-sitter parsing through program graphs to a lattice result.

## Practical starting points

```bash
# Basic recursive evaluation
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

Topos supports Python, Rust, JavaScript, TypeScript, C++, and Go. `evaluate` accepts `--language`, `--preferences`, and `--json`; see [agent and CLI workflows](workflows/agent-and-cli.md) for command and MCP boundaries.

## Read by task

| If you need to… | Read |
| --- | --- |
| Understand parsing, graph construction, engine ownership, and evaluation flow | [Architecture overview](architecture/overview.md) |
| Change a pillar, threshold, security acknowledgement, or role exception | [Quality model](domain/quality-model.md) |
| Change a CLI command, MCP tool contract, agent loop, or advisory output | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| Work on GitNexus, Sighthound, Docker, VS Code, or MCP packaging | [Integrations and distribution](integrations/distribution.md) |
| Run focused verification, modify CI/release code, or ship a build | [Testing and release operations](operations/testing-and-release.md) |
| Find current source and test ownership | [Source map](source-map.md) |

## Current engineering constraints

The Rust engine deliberately normalizes six languages into UAST, then derives CFG, PDG, and CPG views. Deep-tree handling is stack-safe for UAST clone/drop/equality and CFG construction. CFG edge contracts lock selected branch/loop, match/switch-return, and try-return layouts across the complete language registry; preserve these tests when changing traversal.

SIMPLE’s per-function complexity is cross-language UAST based. It now counts decision forms including short-circuit booleans, ternaries, Python comprehension clauses, try handlers, and match/switch arms, so unchanged thresholds apply to a fuller structural model. See [the architecture boundary](architecture/overview.md#simple-complexity-boundary) before changing mappers or gates.

## Repository boundaries

- Source and tests are authoritative. Product documentation lives in `docs/source/`; this wiki explains engineering rationale and maintenance routes.
- `Cargo.toml` owns the workspace version. `scripts/check_versions.py` checks distribution metadata alignment.
- `openwiki/INSTRUCTIONS.md` is a user-authored scope brief; do not rewrite it during normal wiki maintenance.
- The untracked `.github/workflows/openwiki-update.yml` is operational metadata, not Topos runtime code.

## Backlog

- **Formal lattice ordering** — `topos/engine/src/core/omega.rs`: verify the explanatory mathematical ordering against implemented helpers before expanding theory documentation.
- **Parser backend semantics** — `topos/engine/src/graphs/ast/dispatch.rs`: document any additional parser-backend contract only after it is implemented and tested.
