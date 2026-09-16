---
type: maintenance quickstart
title: Topos engineering quickstart
description: Task-routed starting point for maintaining the Topos Rust workspace. Find the owning architecture, quality semantics, workflow, integration, or release guide, then run the narrowest relevant validation.
resource: /README.md
tags: [topos, maintenance, static-analysis, rust, mcp]
openwiki:
  roles: [repository, workflow]
  change_kinds: [cli, mcp, analysis, integration, release]
  source_paths: [Cargo.toml, topos/cli/src/main.rs, topos/engine/src/lib.rs, topos/mcp/src/main.rs]
  test_paths: [topos/cli/tests/install_e2e.rs]
  validation_commands: [cargo test --workspace]
verified:
  - by: openwiki/0.5.2
    at: 2026-09-16T12:21:33.983Z
sources:
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-9a974e970952438ad509f71c
    resource: repo://scripts/check_versions.py
  - id: openwiki-source-4834e4893537d239ae84f8ed
    resource: repo://topos/cli/Cargo.toml
  - id: openwiki-source-fc2ccc731e9f8934a8dd55ae
    resource: repo://topos/cli/src/main.rs
  - id: openwiki-source-06d3c16386c87213458c954c
    resource: repo://topos/cli/tests/install_e2e.rs
  - id: openwiki-source-643b3a33030a101565ff273a
    resource: repo://topos/engine/src/adapters/gitnexus.rs
  - id: openwiki-source-a82b053b744f5ffc408af82c
    resource: repo://topos/engine/src/lib.rs
  - id: openwiki-source-3812b1def9fbad0607404761
    resource: repo://topos/mcp/src/server.rs
  - id: openwiki-source-8680de586193e5fad2de692f
    resource: repo://topos/mcp/tests/lifecycle.rs
generated: { by: "openwiki/0.5.2", at: "2026-09-16T12:21:33.983Z" }
---

# Topos engineering quickstart

Topos is a three-crate Cargo workspace: `topos-engine` owns shared structural analysis and quality classification; `topos` is the human CLI; and `topos-mcp` is the stdio MCP server. Keep policy and representation changes in the engine. The CLI and MCP layers assemble inputs and expose their interface-specific contracts; `topos mcp` launches the same MCP serving implementation as the standalone binary.

Source code and tests are authoritative. Use this page to choose the owner and proof, then use the linked guide for behavior details.

## Start safely

From a clean checkout, confirm the command surface and run a local evaluation that intentionally does not create or refresh GitNexus state:

```bash
cargo run -p topos -- --help
cargo run -p topos -- evaluate . -r --no-composable
```

Run the narrowest relevant test while editing; before merging a shared Rust change, widen to:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For a version, package, plugin, or skill change, also run:

```bash
python3 scripts/check_versions.py
python3 scripts/check_skill.py
python3 scripts/check_agent_plugin.py
```

The root `Cargo.toml` owns the workspace version. Follow [testing, packaging, CI, and release operations](operations/testing-and-release.md) rather than changing release metadata or artifacts in isolation.

## Route the task

| Change or symptom | Read first | Start at | Focused proof |
| --- | --- | --- | --- |
| A pillar, gate, medal, score, preference, suppression, or security acknowledgement | [Four-pillar quality model](domain/quality-model.md) | `topos/engine/src/evaluation/` and `topos/engine/src/core/characteristic_morphism.rs` | `cargo test -p topos-engine <filter>` |
| Parser support, UAST identity, CFG/PDG/CPG construction, a graph metric, or parse behavior | [Rust analysis and evaluation architecture](architecture/overview.md) | `topos/engine/src/graphs/` and `topos/engine/src/functors/` | `cargo test -p topos-engine <filter>` |
| CLI arguments, discovery, terminal/JSON output, or `topos mcp` | [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md) | `topos/cli/src/main.rs`, then `topos/cli/src/commands/` | `cargo test -p topos <filter>` and `cargo run -p topos -- <command> --help` |
| MCP tools, schemas, resources, prompts, protocol lifecycle, assessment, or snapshots | [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md) | `topos/mcp/src/main.rs`, `topos/mcp/src/server.rs`, then the owning router | `cargo test -p topos-mcp --test lifecycle` |
| Install, status, uninstall, harness ownership, pi skill references, or cleanup | [Agent-harness MCP registration lifecycle](workflows/harness-registration.md) | `topos/cli/src/commands/install/` | `cargo test -p topos --test install_e2e` |
| GitNexus, Sighthound, filesystem containment, Docker, VS Code, registry/wheel, plugin, or skill delivery | [Analysis integrations and distribution surfaces](integrations/distribution.md) | The integration boundary named in that guide | Its Rust, Python, or extension check |
| CI admission, installer behavior, version parity, release assets, VSIX, or publishing | [Testing, packaging, CI, and release operations](operations/testing-and-release.md) | `.github/workflows/`, `scripts/`, or the package surface | `python3 scripts/check_versions.py` plus channel-specific checks |
| An owner or regression test not listed here | [Topos maintenance source map](source-map.md) | The row for the observable behavior | The focused check named there |

## High-value boundaries

- **Classify in the engine.** Do not repair a score or verdict in a CLI renderer or MCP formatter before tracing the representation and policy path.
- **A score is not a verdict.** SIMPLE, COMPOSABLE, SECURE, and NAVIGABLE use gate decisions; normalized scores and advisory analyses do not replace those decisions. The quality-model guide also distinguishes parse failure from unavailable optional evidence.
- **COMPOSABLE is optional repository evidence.** It uses a GitNexus-derived module graph. Normal evaluation can retain the other pillars when graph preparation is unavailable; use `topos depgraph generate` when graph setup itself is the task.
- **MCP is a protocol and trust boundary.** Tool schemas and annotations, lifecycle negotiation, and filesystem containment are externally observable. Exercise the real stdio lifecycle for wire changes.
- **Harness registration has narrow ownership.** Preserve foreign configuration and remove only Topos-owned entries. The scratch-home E2E suite is the required safety proof.

## Evaluate and improve

For ordinary local evaluation, use:

```bash
topos evaluate . -r
```

When GitNexus is installed, Topos can prepare the COMPOSABLE graph by default. To make that operation explicit instead:

```bash
topos depgraph generate
topos evaluate . -r
```

For an agent-driven refactor, use the baseline-aware evaluate–edit–assess workflow in [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md). Structural evaluation informs a change; it does not replace the repository's behavior tests, type checks, linters, or release checks.

## Maintenance rule

Update the owning tests whenever a user-visible command or wire contract, security boundary, lifecycle, evaluation decision, or delivery artifact changes. For a cross-cutting symptom, start from the [source map](source-map.md), not from the first renderer that displays it.
