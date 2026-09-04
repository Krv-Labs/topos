---
type: maintenance quickstart
title: Topos code wiki quickstart
description: Task-oriented entry point for maintaining the Topos Rust analysis engine, CLI, MCP server, integrations, and release surfaces. Use the linked behavior guides and focused checks before making a change.
resource: /README.md
tags: [topos, maintenance, static-analysis, rust, mcp]
openwiki:
  roles: [repository, workflow]
  change_kinds: [cli, mcp, analysis, integration, release]
  source_paths: [Cargo.toml, topos/cli/src/main.rs, topos/engine/src/lib.rs, topos/mcp/src/server.rs]
  test_paths: [topos/cli/tests/install_e2e.rs]
  validation_commands: [cargo test --workspace]
verified:
  - by: openwiki/0.5.0
    at: 2026-09-01T16:34:02.929Z
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
generated: { by: "openwiki/0.5.0", at: "2026-09-01T16:34:02.929Z" }
---

# Topos code wiki quickstart

Topos is a three-crate Rust workspace. Keep analysis and policy changes in `topos-engine`; the `topos` CLI and `topos-mcp` stdio server are consumer and delivery layers. The CLI also starts that same MCP server with `topos mcp`. Start with the task route below rather than repairing a symptom in a renderer or protocol response.

## First local checks

Run the narrowest check that proves the changed contract, then widen it for shared code or a merge-ready change:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For a public CLI behavior, smoke-test the built command as well:

```bash
cargo run -p topos -- <command> --help
```

The root `Cargo.toml` owns the workspace version. For versioned or packaged changes, follow the metadata and channel checks in [testing and release operations](operations/testing-and-release.md).

## Route the change

| If you need to change… | Read first | Start at | Focused proof |
| --- | --- | --- | --- |
| A quality pillar, gate, threshold, score, preference, suppression, or medal | [Four-pillar quality model](domain/quality-model.md) | `topos/engine/src/evaluation/`, then `topos/engine/src/core/characteristic_morphism.rs` | `cargo test -p topos-engine <filter>` |
| Parsing, a supported language, UAST identity, CFG/PDG/CPG construction, or a structural metric | [Rust analysis and evaluation architecture](architecture/overview.md) | `topos/engine/src/graphs/` and `topos/engine/src/functors/` | `cargo test -p topos-engine <filter>` |
| CLI commands, arguments, JSON/terminal presentation, or `topos mcp` | [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md) | `topos/cli/src/main.rs`, then `topos/cli/src/commands/` | `cargo test -p topos <filter>` |
| An MCP tool, resource, prompt, router, protocol lifecycle, assessment, or agent loop | [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md) | `topos/mcp/src/server.rs`, then `topos/mcp/src/tools/` | `cargo test -p topos-mcp`; use `cargo test -p topos-mcp --test lifecycle` for wire-surface changes |
| Installing, inspecting, or removing an agent-harness registration | [Agent-harness MCP registration lifecycle](workflows/harness-registration.md) | `topos/cli/src/commands/install/` | `cargo test -p topos --test install_e2e` |
| GitNexus/COMPOSABLE, Sighthound, MCP path containment, Docker, VS Code, plugin, skill, or registry packaging | [Analysis integrations and distribution surfaces](integrations/distribution.md) | The integration boundary named in that guide | Run its integration-specific Rust, Python, or extension check |
| CI admission, release assets, installer, version parity, wheel, VSIX, or publishing | [Testing, packaging, CI, and release operations](operations/testing-and-release.md) | `.github/workflows/`, `scripts/`, or the relevant package surface | `python3 scripts/check_versions.py` plus the guide’s channel-specific check |
| An owner or test location not listed here | [Topos maintenance source map](source-map.md) | The source-map row for the observable behavior | The focused check named by that row |

## Non-negotiable boundaries

- **Classify in the engine.** The engine owns program representations and four-pillar evaluation; CLI and MCP should assemble inputs, apply their interface contracts, and render or transport results. Read the [architecture overview](architecture/overview.md) before changing representation assembly.
- **Do not confuse a score with a verdict.** SIMPLE, COMPOSABLE, SECURE, and NAVIGABLE are independently evaluated; the quality-model guide distinguishes decisive raw gates, optional evidence, normalized reporting scores, and advisory analyses.
- **Treat COMPOSABLE as optional topology evidence.** It requires a GitNexus-derived dependency graph. If that graph is unavailable, preserve the other analysis results and diagnose the graph path rather than declaring an ordinary evaluation unusable.
- **Treat MCP as a wire and trust boundary.** Router registration, `tools/list` schemas and annotations, resources/prompts, negotiated protocol versions, and filesystem containment are externally observable. Test the real stdio lifecycle when changing those surfaces.
- **Keep registration ownership narrow.** Installer changes must preserve foreign harness configuration and only remove entries Topos recognizes as its own; use the scratch-home end-to-end suite.

## Common operating routes

### Evaluate locally

```bash
topos evaluate src/ -r
```

To prepare cross-module COMPOSABLE evidence, install the tested GitNexus version, generate the graph, and supply its directory:

```bash
npm install -g gitnexus@1.6.8
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus
```

For an agent-led change, use the baseline-aware evaluate–edit–assess route in [CLI, MCP, and agent improvement workflows](workflows/agent-and-cli.md). Structural coverage and comparison commands are advisory: they inform a change but do not change the four-pillar verdict.

### Change a shipped surface

Before changing an integration or release artifact, identify which host launches which binary and what path/trust configuration it supplies. In particular, file-oriented MCP tools have a project-discovery and containment boundary, and GitNexus generation is a subprocess/store integration rather than a replacement parser. The [distribution guide](integrations/distribution.md) documents both boundaries; the [operations guide](operations/testing-and-release.md) gives the proof required for each delivery channel.

## Maintenance rule

Source code and tests are authoritative. This page routes work; use the linked guide for behavior details and the [source map](source-map.md) to find the owning implementation and regression coverage. Update the owning tests whenever a user-visible contract, security boundary, lifecycle, or evaluation decision changes.
