---
type: quickstart
title: Quickstart Guide
description: Immediate onboarding entry point, workspace CLI/MCP command references, and task-based navigation map across all OpenWiki sections for Topos maintainers and AI agents.
tags: [topos, quickstart, rust, cli, mcp, navigation]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-30T12:01:57.716Z
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-9a974e970952438ad509f71c
    resource: repo://scripts/check_versions.py
  - id: openwiki-source-4834e4893537d239ae84f8ed
    resource: repo://topos/cli/Cargo.toml
  - id: openwiki-source-3e71a723bc6b684f48f3f2f7
    resource: repo://topos/cli/src/commands/update/mod.rs
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
generated: { by: "openwiki/0.7.2", at: "2026-10-30T12:01:57.716Z" }
---

# Quickstart Guide

Topos is a three-crate Cargo workspace: `topos-engine` owns shared structural analysis, categorical model representations, and quality classification; `topos` provides the human CLI interface and command dispatch; and `topos-mcp` implements the stdio Model Context Protocol (MCP) server. Keep domain policy and structural analysis in the engine crate. The CLI and MCP crates wrap engine capabilities into their respective transport interfaces; `topos mcp` launches the same in-process MCP server implementation as the standalone `topos-mcp` binary.

Source code and tests are authoritative. Use this quickstart to route maintenance tasks, apply agent guidelines, run targeted verification, and navigate the OpenWiki documentation suite across architecture, domain concepts, workflows, operations, source map, and integrations.

## Workspace Architecture

The root `Cargo.toml` defines the workspace package version (`0.15.0`) and contains three member crates:

- **`topos-engine`**: Pure-Rust evaluation core containing configuration (`.topos.toml`), categorical primitives (`core`), AST/CFG/CPG/PDG/MDG/UAST representations (`graphs`), metrics/functors (`functors`), quality translators (`evaluation`), and adapters (`adapters`).
- **`topos`**: Standalone CLI binary (`src/main.rs`) dispatching 13 root subcommands: `evaluate`, `inspect`, `pr-recap`, `config`, `compare`, `coverage`, `depgraph`, `cache`, `install`, `uninstall`, `update`, `status`, and `mcp`. On failure, commands print an error message to stderr and exit with status 1.
- **`topos-mcp`**: Stdio MCP server combining evaluate, assess, compare, coverage, depgraph, docs, inspect, preferences, pr-recap, and refactor tool routers, embedded documentation resources (`topos://docs/*`), build identity resource (`topos://build`), and the `topos_refactor_until_ideal` prompt.

```mermaid
flowchart TD
    subgraph Workspace ["Cargo Workspace (v0.15.0)"]
        Engine["topos-engine (Evaluation Core and Categorical Primitives)"]
        CLI["topos CLI (Command Dispatcher and Human UI)"]
        MCP["topos-mcp (Stdio MCP Server)"]
    end

    subgraph Entrypoints
        Developer["Developer / Maintainer"]
        Agent["AI Agent / Harness"]
        VSCode["VS Code Extension"]
    end

    Developer -->|cargo run -p topos| CLI
    Agent -->|stdio JSON-RPC| MCP
    VSCode -->|topos mcp| CLI
    CLI -->|bundles / launches| MCP
    CLI -->|dispatches to| Engine
    MCP -->|calls evaluation core| Engine
```
*Topos workspace component relationships, entrypoints, and communication channels.*

## Start Safely

From a clean repository checkout, verify the CLI binary and run a baseline evaluation that disables automatic GitNexus background graph generation:

```bash
cargo run -p topos -- --help
cargo run -p topos -- evaluate . -r --no-composable
```

Use the narrowest relevant test during development, then expand validation before committing or opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For package, version, skill, or agent plugin updates, also run the Python verification scripts:

```bash
python3 scripts/check_versions.py
python3 scripts/check_skill.py
python3 scripts/check_agent_plugin.py
```

## Route the Task

When undertaking maintenance tasks across the Topos workspace, use the routing map and diagram below to identify target documentation, starting source directories, and focused verification proof commands.

```mermaid
flowchart TD
    Start["Maintenance Task or Symptom"] --> Choice{"Which area is affected?"}
    
    Choice -->|"Engine & Categorical Core"| EngineTask["Engine Domain & Metrics (topos/engine/src/)"]
    Choice -->|"CLI Subcommands & Terminal UI"| CLITask["CLI Commands & Dispatch (topos/cli/src/)"]
    Choice -->|"MCP Server & JSON-RPC"| MCPTask["MCP Routers & Lifecycle (topos/mcp/src/)"]
    Choice -->|"Harnesses & Registration"| InstallTask["Harness Registration & Setup (topos/cli/src/commands/install/)"]
    Choice -->|"Release, Versioning & CI"| ReleaseTask["Distribution & CI Workflows (scripts/, .github/workflows/)"]

    EngineTask --> EngineProof["cargo test -p topos-engine filter"]
    CLITask --> CLIProof["cargo test -p topos filter"]
    MCPTask --> MCPProof["cargo test -p topos-mcp --test lifecycle"]
    InstallTask --> InstallProof["cargo test -p topos --test install_e2e"]
    ReleaseTask --> ReleaseProof["python3 scripts/check_versions.py"]
```
*Task routing flowchart linking system layers to primary source paths and focused verification commands.*

| Task or Symptom | Read First | Start Location | Focused Proof |
| --- | --- | --- | --- |
| Pillars (`SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE`), gate decisions, thresholds, suppressions, preferences | [Quality Model & Domain Concepts](domain/quality-model.md) | `topos/engine/src/evaluation/`, `topos/engine/src/core/` | `cargo test -p topos-engine <filter>` |
| Structural views (AST, UAST, CFG, PDG, CPG, MDG), graph metrics, categorical functors, curvature | [System Architecture Overview](architecture/overview.md) | `topos/engine/src/graphs/`, `topos/engine/src/functors/` | `cargo test -p topos-engine <filter>` |
| CLI subcommands (`evaluate`, `inspect`, `pr-recap`, `config`, `compare`, `coverage`, `depgraph`, `cache`, `install`, `uninstall`, `update`, `status`, `mcp`), terminal rendering, JSON output | [Agent & CLI Workflows](workflows/agent-and-cli.md) | `topos/cli/src/main.rs`, `topos/cli/src/commands/` | `cargo test -p topos <filter>` and `cargo run -p topos -- <cmd> --help` |
| MCP tools, JSON-RPC schemas, resources, prompts, stdio transport lifecycle, MCP protocol versions | [Agent & CLI Workflows](workflows/agent-and-cli.md) | `topos/mcp/src/main.rs`, `topos/mcp/src/server.rs` | `cargo test -p topos-mcp --test lifecycle` |
| Agent harness registration (`install`, `uninstall`, `status`), harness configs (`~/.claude.json`, Codex, Gemini), clean-home residue | [Harness & Plugin Registration Workflow](workflows/harness-registration.md) | `topos/cli/src/commands/install/` | `cargo test -p topos --test install_e2e` |
| GitNexus (`.gitnexus` store), distribution channels (binary installer `install.sh`, Homebrew, Cargo, Python wheel, VS Code VSIX, agent skills/plugins), updates | [Integration & Distribution Models](integrations/distribution.md) | `topos/engine/src/adapters/gitnexus.rs`, `topos/cli/src/commands/update/`, `install.sh` | `python3 scripts/check_skill.py` and channel-specific checks |
| CI pipelines, release packaging, version consistency, release tags, publishing workflows | [Testing, CI/CD, & Release Runbook](operations/testing-and-release.md) | `.github/workflows/`, `scripts/check_versions.py` | `python3 scripts/check_versions.py` |
| Complete codebase file structure, cross-crate module paths, script locations | [Source Code Map & Module Guide](source-map.md) | Target source file named in the map | Relevant crate or module unit tests |

## Agent Interaction Policies

When working as an AI agent or maintainer in the Topos repository, adhere to the guidelines defined in `AGENTS.md`:

- **Just-in-Time Documentation Retrieval**: Do not enumerate, preload, or search wikis at task start. Retrieve documentation only when requested or when unfamiliar architecture materially impacts the task. If MCP retrieval tools are unavailable, read `quickstart.md` and follow relative links.
- **Authoritative Sources**: Source code and tests are primary authority. Briefs or documentation notes represent verification gaps, not automatic requirements.
- **Quiet, Targeted Validation**: Prefer the narrowest quiet validation command that proves the changed behavior. Always preserve complete failure output when tests fail.
- **Skill and Plugin Parity**: Synchronize versions and content across `skills/topos/SKILL.md`, `agent-plugin/`, `.mcp/server.json`, `extensions/vscode/package.json`, and `Cargo.toml`. Validate changes using `python3 scripts/check_versions.py`, `python3 scripts/check_skill.py`, and `python3 scripts/check_agent_plugin.py`.
- **OpenWiki CI Policy**: The `.github/workflows/openwiki.yml` pipeline regenerates engineering documentation from source on `main` branch pushes or manual dispatch, submitting documentation PRs without modifying workflow files directly.
- **Embedded MCP Docs vs Workspace Wiki**: Embedded MCP resources (`topos_get_doc` / `topos://docs/*`) serve six specific topics (`agent-contract`, `lattice`, `metrics`, `preferences`, `priority`, `workflows`). Broader workspace engineering guides live under `openwiki/` on the filesystem and are read directly by agents with repository access.

## Update Mechanics and Checking Options

The `topos update` command inspects the binary's installation channel and reports available upgrades:

- **Distribution Channels**: Identifies whether the running binary was installed via binary download (`install.sh`), Homebrew (`brew install krv-labs/tap/topos`), Cargo (`cargo install`), or source checkout (`cargo build`).
- **Command Options**:
  - `topos update --check`: Reports installed vs published versions without making changes or prompting. Implied in non-terminal environments.
  - `topos update --yes` (`-y`): Applies channel-specific upgrades non-interactively without confirmation prompts.
  - `topos update --json`: Emits the update survey as machine-readable JSON.
- **Passive Notices & Throttling**: Interactive CLI invocations execute a throttled passive update check at most once every 24 hours, caching results in `~/.local/state/topos`. Set `TOPOS_NO_UPDATE_NOTICES=1` to silence passive update notices completely.

## High-Value Engineering Invariants

- **Classification Belongs in the Engine**: Quality evaluations, metric scores, and gate verdicts MUST be computed within `topos-engine`. Never override or patch quality verdicts in CLI renderers or MCP formatters.
- **Scores Are Not Gate Verdicts**: Quality metrics yield normalized float scores, but gate decisions (`SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE`) are Heyting algebra elements evaluated against project priority thresholds.
- **COMPOSABLE Graph Independence**: COMPOSABLE uses GitNexus module graphs. If GitNexus is uninstalled or graph generation fails, standard evaluation continues operating on `SIMPLE`, `SECURE`, and `NAVIGABLE` pillars. Use `topos depgraph generate` or `topos cache` to manage graph generation and cached structural artifacts.
- **MCP Wire Safety**: `topos-mcp` explicitly bounds protocol version support (`2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`, `2026-07-28`) and isolates filesystem roots. Always run `cargo test -p topos-mcp --test lifecycle` when modifying MCP tool schemas, resources, or lifecycle handlers.
- **Harness Clean-Up**: `topos install` and `topos uninstall` must preserve non-Topos configuration entries in `~/.claude.json`, Codex TOML, and Gemini settings. Running `cargo test -p topos --test install_e2e` verifies that installation and uninstallation leave no unwanted residue in `$HOME`.
