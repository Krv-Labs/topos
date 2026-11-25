---
type: maintenance source map
title: Source Code Map & Module Guide
description: Ownership-oriented starting points and directory layouts for Topos engine analysis, CLI delivery, MCP servers, integration extensions, test suites, and operational scripts.
tags: [source-map, maintenance, navigation, rust, directory-layout]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-25T22:07:40.661Z
sources:
  - id: openwiki-source-164e2da859b5277df81c7d94
    resource: repo://.github/workflows/ci.yml
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-1ff55ce1c8213af5491772db
    resource: repo://extensions/vscode/src/extension.ts
  - id: openwiki-source-9a974e970952438ad509f71c
    resource: repo://scripts/check_versions.py
  - id: openwiki-source-25ff351f2be56b85dd408a14
    resource: repo://scripts/ci_gate.py
  - id: openwiki-source-109b3cc8dcd24a7fc78b0a8d
    resource: repo://tests/packaging/test_install_sh_preflight.py
  - id: openwiki-source-9151a756a3b86237bb593dce
    resource: repo://topos/cli/src/commands/install/harness.rs
  - id: openwiki-source-1acdcf52cfb4a8b45468a39a
    resource: repo://topos/cli/src/commands/install/mod.rs
  - id: openwiki-source-90c02b6898554ee82e523723
    resource: repo://topos/cli/src/commands/mcp.rs
  - id: openwiki-source-fc2ccc731e9f8934a8dd55ae
    resource: repo://topos/cli/src/main.rs
  - id: openwiki-source-06d3c16386c87213458c954c
    resource: repo://topos/cli/tests/install_e2e.rs
  - id: openwiki-source-c386b4993bec7b31315a096c
    resource: repo://topos/engine/src/config.rs
  - id: openwiki-source-a82b053b744f5ffc408af82c
    resource: repo://topos/engine/src/lib.rs
  - id: openwiki-source-ca5302a9c887c6fd31583237
    resource: repo://topos/mcp/src/evaluation/mod.rs
  - id: openwiki-source-9bf1d8e64277056e6ccedf90
    resource: repo://topos/mcp/src/main.rs
  - id: openwiki-source-416dcb63c9e3e0c0c2cb0eed
    resource: repo://topos/mcp/src/security.rs
  - id: openwiki-source-3812b1def9fbad0607404761
    resource: repo://topos/mcp/src/server.rs
  - id: openwiki-source-a9e2b99472d2a9efbdb51629
    resource: repo://topos/mcp/src/tools/depgraph.rs
  - id: openwiki-source-8680de586193e5fad2de692f
    resource: repo://topos/mcp/tests/lifecycle.rs
generated: { by: "openwiki/0.9.0", at: "2026-11-25T22:07:40.661Z" }
---

# Topos maintenance source map

Use this as a change-planning map and directory layout reference for the Topos repository. The root Cargo workspace (`Cargo.toml`) contains three Rust crates: `topos-engine` (`topos/engine`) as the transport-free analysis core, `topos` (`topos/cli`) as the human CLI binary and local MCP launcher, and `topos-mcp` (`topos/mcp`) as the stdio MCP server implementation. `Cargo.toml` serves as the release-version authority across the workspace (version `0.16.0`).

For broader context, see the [Architecture Overview](architecture/overview.md), [Quickstart Guide](quickstart.md), and [Testing and Release Operations](operations/testing-and-release.md).

```mermaid
flowchart TD
    subgraph CargoWorkspace ["Cargo Workspace Root (Cargo.toml)"]
        Engine ["topos-engine (topos/engine)"]
        CLI ["topos (topos/cli)"]
        MCP ["topos-mcp (topos/mcp)"]
    end

    subgraph Integrations ["Integrations & Extensions"]
        VSCode ["VS Code Extension (extensions/vscode)"]
        AgentPlugin ["Agent Plugin (agent-plugin)"]
    end

    subgraph PackagingTooling ["Packaging & Scripts"]
        InstallSh ["install.sh & packaging/"]
        Scripts ["scripts/ (CI & Version Tools)"]
    end

    subgraph TestSuites ["Test Suites"]
        CLITests ["topos/cli/tests/ (CLI Integration)"]
        MCPTests ["topos/mcp/tests/ (MCP Lifecycle)"]
        PkgTests ["tests/packaging/ (Pytest Preflight)"]
    end

    CLI -->|"Direct Analysis Calls"| Engine
    CLI -->|"topos mcp (In-process MCP launch)"| MCP
    MCP -->|"Evaluation & Metrics"| Engine
    VSCode -->|"Stdio Launch (topos mcp)"| CLI
    AgentPlugin -->|"Stdio Launch (topos mcp)"| CLI
    InstallSh -->|"Installs CLI Binary"| CLI
    Scripts -->|"Enforces Version & CI Gates"| CargoWorkspace
    CLITests -->|"Verifies CLI Subcommands"| CLI
    MCPTests -->|"Verifies Stdio JSON-RPC"| MCP
    PkgTests -->|"Verifies Shell Installer"| InstallSh
```
Workspace crate boundaries, integration points, scripts, and test suites.

## Directory and file layout

### 1. `topos/engine` (`topos-engine` crate)
The core category-theoretic compute engine for Topos. It contains no CLI or MCP transport code and no Python bindings, acting as a pure library linked by both `topos` and `topos-mcp`.

- `src/lib.rs`: Engine entry point; exposes core modules (`adapters`, `config`, `core`, `evaluation`, `functors`, `graphs`).
- `src/core/`: Mathematical category primitives and the subobject classifier.
  - `category.rs`: Category structures and functor interfaces.
  - `morphism.rs`: Structure-preserving transformations.
  - `object.rs`: Core object representations (ASTs, program graphs).
  - `omega.rs`: Subobject classifier $\Omega$ (free Heyting algebra built on `SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE`).
  - `characteristic_morphism.rs`: Characteristic morphism $\chi_S: P \to \Omega$.
- `src/graphs/`: Graph structures and Universal AST representations.
  - `base.rs`: Shared `Representation` trait protocol.
  - `ast/`: Language AST objects (`languages.rs`, `types.rs`, `dispatch.rs`, `object.rs`).
  - `cfg/`: Control Flow Graphs (`builder.rs`, `models.rs`, `edge_contracts.rs`, `object.rs`).
  - `cpg/`: Code Property Graphs (`builder.rs`, `models.rs`, `object.rs`).
  - `mdg/`: Module Dependency Graphs (`file_graph.rs`, `ladybug.rs`, `json.rs`, `split.rs`, `edge_filter/` language syntax rules for JS, JSONC, Python, Rust, paths).
  - `pdg/`: Program Dependency Graphs (`object.rs`, `dataflow.rs`).
  - `process/`: Process execution models (`object.rs`).
  - `uast/`: Universal AST mapping for supported languages (`mapper_rust.rs`, `mapper_python.rs`, `mapper_javascript.rs`, `mapper_typescript.rs`, `mapper_go.rs`, `mapper_cpp.rs`, `mapper_common.rs`, `models.rs`).
- `src/functors/`: Metrics (probes) and cross-representation profunctors.
  - `curvature.rs`: Forman-Ricci sectional curvature computation.
  - `probes/`: Metric probes over representations (AST: `complexity`, `entropy`, `scopes`, `divergence`; CFG: `homology`; CPG: `taint`, `danger`; MDG: `fan`, `coupling`, `curvature`; UAST: `abstractness`, `signature`; process: `curvature`).
  - `profunctors/`: Comparison profunctors (AST, CFG, CPG, MDG, PDG, UAST: `structural_test_coverage`, `inline_tests`, `ledger`, `compare`).
- `src/evaluation/`: Quality evaluation and policy mapping.
  - `policies/`: Quality pillar policy translators (`simple.rs`, `composable.rs`, `secure.rs`, `navigable.rs`, `advisory_priors.json`, `clones.rs`, `coverage.rs`, `desirability.rs`, `gates.rs`, `calibration.rs`).
  - Policy & guidance helpers: `advisory.rs`, `file_roles.rs`, `preferences.rs`, `security_guidance.rs`, `suggestions.rs`, `suppression.rs`, `waivers.rs`.
- `src/adapters/`: System and discovery interfaces.
  - `discovery.rs`: Source tree scanner and file discovery.
  - `gitnexus.rs`: GitNexus dependency graph invocation and store reader.
  - `process.rs`: Internal process execution utility.
- `src/config.rs` & `src/config/`: Configuration reader (`.topos.toml`), allowlist logic (`ToposConfig`), and PR gate rules (`pr_gate.rs`).

### 2. `topos/cli` (`topos` crate)
The CLI application and in-process MCP launcher binary.

- `src/main.rs`: Command line parsing and dispatch (`Cli`, `Command` enum, `pre_dispatch()`, `run_command()`, `wants_no_notice()`, `dispatch()`). Defines the 14 user-facing subcommands: `evaluate`, `inspect`, `pr-recap`, `config`, `compare`, `coverage`, `badge`, `depgraph`, `cache`, `install`, `uninstall`, `update`, `status`, `mcp`.
- `src/commands/`: Command modules and helpers.
  - Subcommand directories: `evaluate/`, `inspect/`, `pr_recap/`, `badge/`, `depgraph/`, `install/`, `update/`.
  - Single-file subcommands: `config.rs`, `compare.rs`, `coverage.rs`, `cache.rs`, `mcp.rs`.
  - Harness installation modules (`commands/install/`): `harness.rs` (10-row harness table), `artifact.rs`, `binary.rs`, `configure.rs`, `fsops.rs`, `json_entry.rs`, `paths.rs`, `report.rs`, `residue.rs`, `skills_entry.rs`, `state.rs`, `status.rs`, `testing.rs`, `toml_entry.rs`, `uninstall.rs`.
  - Shared CLI utilities: `classify.rs`, `composable.rs`, `gh.rs`, `interaction.rs`, `lang.rs`, `menu.rs`, `render.rs`.
- `tests/`: Integration test suites for the CLI.
  - `evaluate_e2e.rs`, `install_e2e.rs`, `badge_e2e.rs`, `cache_e2e.rs`, `composable_e2e.rs`, `coverage_cli.rs`, `pr_recap_e2e.rs`, `pr_recap_entropy_e2e.rs`, `pr_recap_new_code_e2e.rs`, `update_e2e.rs`.

### 3. `topos/mcp` (`topos-mcp` crate)
The stdio Model Context Protocol (MCP) server implementation and standalone executable.

- `src/lib.rs`: Exposes MCP server components.
- `src/main.rs`: Entrypoint for standalone binary (`topos-mcp`).
- `src/server.rs`: Central `ToposServer` stdio JSON-RPC handler; aggregates tool routers, serves static resources, and handles protocol initialization.
- `src/tools/`: Router modules tagged with `#[tool_router]`.
  - `assess.rs`: Project snapshots and structure assessment.
  - `compare.rs`: Two-file structural comparison.
  - `coverage.rs`: Test structural coverage analysis.
  - `depgraph.rs`: COMPOSABLE graph and GitNexus operations.
  - `docs.rs`: In-server documentation tool.
  - `evaluate/`: Multi-file evaluation (`mod.rs`, `file_targets.rs`, `large_tree.rs`, `summary.rs`, `worst.rs`).
  - `inspect.rs`: Single-file deep inspection tool.
  - `preferences.rs`: User preference and priority tools.
  - `refactor.rs`: Refactor hotspots and curvature analysis.
- Supporting MCP modules:
  - `schemas.rs`, `formatting.rs`: MCP JSON-RPC schemas and response formatting.
  - `security.rs`, `security_findings.rs`, `sighthound.rs`, `diagnostics.rs`: Path containment, project root resolution (`TOPOS_MCP_FILE_ROOT`), and security overlays.
  - `evaluation/`, `cache/`, `pr_recap/`, `update/`: MCP-specific evaluation bridge, cache discovery/lease management, PR recap engine, and update notification channels.
  - `docs/content/`: Static embedded documentation (`agent-contract.md`, `lattice.md`, `metrics.md`, `preferences.md`, `priority.md`, `workflows.md`).
- `tests/`: Integration test suites for the MCP server.
  - `lifecycle.rs`: Wire-level stdio JSON-RPC tests covering initialization, capabilities, discovery, and protocol negotiation.
  - `path_resolution.rs`: Security boundary and path containment validation.
  - `evaluate_project_summary.rs`, `coverage.rs`, `tasks.rs`: Tool behavior integration tests.

### 4. `extensions/` & `agent-plugin/`
Integrations for agent environments and IDE host applications.

- `extensions/vscode/`: VS Code Extension package.
  - `src/extension.ts`: Registers stdio MCP definitions, command palette workflows, and status indicators. Sets `TOPOS_MCP_FILE_ROOT` to host workspace.
  - `src/runtime.ts`: Ordered binary runtime resolution (configured path, bundled binary, cache, `PATH`, virtual environments, verified download).
- `agent-plugin/`: Agent Plugin distribution package.
  - `plugin.json`: Plugin metadata and identity declaration.
  - `mcp.json`: Stdio MCP registration executing `topos mcp`.
  - `skills/topos/SKILL.md`: Distributed skill definition and agent guidance.

### 5. `packaging/` & `install.sh`
Distribution package definitions, formulas, and shell installer.

- `install.sh`: POSIX shell installer handling target binary selection, SHA-256 verification, in-place upgrade preflight checks, Homebrew detection, and non-interactive execution flags (`TOPOS_FORCE`/`TOPOS_YES`).
- `packaging/homebrew/topos.rb.template`: Homebrew formula template populated during release generation.

### 6. `scripts/`
Repository maintenance, release validation, and CI gating scripts.

- `scripts/check_versions.py`: Enforces version parity across `Cargo.toml`, VS Code extension `package.json`, `agent-plugin/plugin.json`, and `.mcp/server.json`.
- `scripts/ci_gate.py`: Gating script for GitHub Actions CI runs, evaluating trunk and stacked PR status while failing open on malformed/missing stack payloads.
- `scripts/check_agent_plugin.py`: Validates agent plugin manifest structure and file presence.
- `scripts/check_skill.py`: Validates agent skill payload integrity and frontmatter structure.
- `scripts/setup-lbug-prebuilt.sh`: Downloads prebuilt `liblbug` static libraries for Cargo linking before workspace builds.

### 7. `tests/`
Root integration test fixtures and packaging test suites.

- `tests/fixtures/composable_mdg/`: Multi-file Rust project fixture (`Cargo.toml`, `README.md`, `src/lib.rs`, `src/helper.rs`) used by COMPOSABLE MDG tests.
- `tests/packaging/`: Packaging test suites.
  - `test_install_sh_preflight.py`: Pytest suite verifying shell installer (`install.sh`) preflight checks, in-place upgrades, non-interactive piping, and Homebrew coexistence.

## Owned systems

| Change concern | Ownership and starting points | Focused checks and decisions | Behavior reference |
| --- | --- | --- | --- |
| **Quality model and verdicts** | `topos/engine/src/core/` defines the categorical primitives, four-generator `Omega`, and characteristic morphism. `topos/engine/src/evaluation/` translates representation evidence into policies, preferences, suggestions, and suppression. Change this layer before changing CLI or MCP rendering. | Keep the four pillars (SIMPLE, COMPOSABLE, SECURE, NAVIGABLE) coherent across policy, preference, and output. `topos/engine/src/config.rs` loads the nearest `.topos.toml`; malformed or absent configuration falls back to an empty config, while acknowledged security allowlist entries require non-empty `pattern` and `reason` values and remain disclosed rather than silently improving the canonical verdict. | [Quality model](domain/quality-model.md) |
| **Parsing and structural representations** | `topos/engine/src/graphs/` owns AST, UAST, CFG, CPG, PDG, MDG, and process representations; `graphs/base.rs` is the shared representation protocol. `topos/engine/src/functors/` owns probes and cross-representation profunctors. | Put a parser/normalization regression beside its responsible graph or probe module. For cross-language UAST shape and provenance decisions, consult `docs/decisions/uast-industry-standards.md`; CFG control-flow semantics have adjacent module tests. | [Architecture overview](architecture/overview.md) |
| **Dependency topology and COMPOSABLE** | `topos/engine/src/adapters/gitnexus.rs` owns GitNexus invocation/store handling; `graphs/mdg/` interprets module dependency data. The CLI `commands/depgraph/` and MCP `tools/depgraph.rs` expose build/status operations. | COMPOSABLE is git-root scoped: do not derive the `.gitnexus` store from a nested Cargo package. The CI `composable` job installs pinned GitNexus, analyzes a fixture, and asserts that `evaluate` emits a non-null COMPOSABLE score rather than silently taking the missing-store path. The file-level gate measures outward dependency burden; detailed topology remains advisory. | [Quality model](domain/quality-model.md) and [Integrations and distribution](integrations/distribution.md) |
| **CLI commands and presentation** | `topos/cli/src/main.rs` is the Clap dispatch boundary, owning fourteen user-facing subcommands (`evaluate`, `inspect`, `pr-recap`, `config`, `compare`, `coverage`, `badge`, `depgraph`, `cache`, `install`, `uninstall`, `update`, `status`, `mcp`). `commands/evaluate/` and `commands/inspect/` adapt engine outcomes for users; `compare.rs`, `coverage.rs`, `badge.rs`, and `composable.rs` provide advisory structural workflows. `commands/mcp.rs` starts `topos_mcp::server::serve` on a Tokio runtime. | Preserve the distinction between lattice evaluation and advisory comparison/coverage. A bare CLI exits with help; command errors are printed and exit non-zero. Add unit coverage near the command for parsing, output, and error behavior. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| **Harness registration** | `topos/cli/src/commands/install/` owns the harness table, config-format adapters, absolute-binary resolution, atomic filesystem work, state inspection, and uninstall cleanup. `harness.rs` is the ten-row extension point: ordinary harnesses have one Topos-owned MCP registration; `pi` is the narrow exception because it can also receive a reference to an already-installed skill directory. | Do not write instruction blocks, `@import` lines, or skill contents. Non-interactive install needs explicit harnesses or `--all`; destructive uninstall differentiates interactive, headless, and ambiguous stream arrangements. Run `cargo test -p topos --test install_e2e`: the real-binary scratch-home suite protects round-trip cleanup, foreign configuration preservation, exit behavior, and JSON reporting. `docs/decisions/cli-harness-install.md` is the compatibility and format decision record. | [Agent-harness registration](workflows/harness-registration.md) |
| **MCP protocol, agent tools, and state** | `topos/mcp/src/main.rs` is the standalone stdio binary; `server.rs` combines the routers in `tools/` and directly serves static docs/resources and the refactor prompt. Tool modules own evaluation, assessment/snapshots, comparison, coverage, dependency graphs, documentation, inspection, preferences, PR recaps, and refactor targets. `schemas.rs` and `formatting.rs` own the agent-facing wire shape. | Do not add a protocol revision implicitly: `server.rs` pins the negotiated revisions. Keep initialize and stateless discovery on the same tool surface. `topos/mcp/tests/lifecycle.rs` pipes JSON-RPC into the built binary to exercise both lifecycles and each supported initialize revision. `topos://build` is the low-context diagnostic resource for the serving executable, root, and staleness. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| **MCP filesystem and security evidence** | `topos/mcp/src/security.rs` owns path containment and project/root discovery; `security_findings.rs`, `sighthound.rs`, and `diagnostics.rs` normalize security evidence and overlays. | Filesystem tools must resolve an existing path, discover a project marker, and fail closed outside `TOPOS_MCP_FILE_ROOT` when it is configured. Without that variable, callers must provide an absolute path or rely on relative-path resolution against the startup project root. Preserve incremental symlink resolution for missing graph-store leaves so a link cannot escape the boundary. Run `cargo test -p topos-mcp security` for containment regressions. | [MCP launch and filesystem trust boundary](integrations/distribution.md#mcp-server-launch-and-filesystem-trust-boundary) |
| **VS Code integration** | `extensions/vscode/src/extension.ts` registers the MCP definition provider and Command Palette workflows. `runtime.ts` owns invocation construction, runtime discovery/download, manifest selection, checksums, redirects, and timeouts. | The extension passes its workspace as `TOPOS_MCP_FILE_ROOT`, so it participates in the MCP access boundary. Runtime resolution is ordered: configured executable, bundled binary, cache, PATH, optional virtual-environment discovery, then optional verified download. Run `pnpm run check-types`, `pnpm run lint`, and `pnpm run test:unit`; integration tests are separately available. | [Integrations and distribution](integrations/distribution.md) |
| **Agent Plugin and skill package** | `agent-plugin/plugin.json` is package metadata; `agent-plugin/mcp.json` is its stdio registration and invokes `topos mcp`; `agent-plugin/skills/topos/` holds the distributed skill. | Run `scripts/check_agent_plugin.py` and `scripts/check_skill.py` after changing package or skill material. Keep plugin metadata and registration distinct from the CLI installer, which changes local harness configurations. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| **Binary installer and Homebrew coexistence** | `install.sh` owns release-binary selection, checksum verification, target installation, PATH guidance, and existing-install preflight. Treat package-manager installs as separate channels rather than as CLI update/uninstall behavior. | Run `pytest tests/packaging/test_install_sh_preflight.py`. Preserve the same-target in-place upgrade, channel-specific Homebrew hint, and non-blocking behavior for non-interactive or piped input; `TOPOS_FORCE` / `TOPOS_YES` deliberately bypass the interactive confirmation. | [Testing and release operations](operations/testing-and-release.md#installer-and-homebrew-checks) |
| **CI, publication, and versioning** | `.github/workflows/ci.yml` is the admission and verification workflow; `scripts/ci_gate.py` owns the stacked-PR decision. `.github/workflows/release.yml` builds native artifacts, packages VSIX targets, publishes releases, and handles downstream publication. `scripts/check_versions.py` checks published metadata against the workspace version. | The CI gate runs trunk and stacked PRs; unavailable or malformed stack data fails open so verification is not silently skipped. The Rust job runs formatting, warning-denying Clippy, workspace tests, package/skill checks, and a stdio MCP smoke test. Version changes begin in root `Cargo.toml`; `check_versions.py` requires the VS Code package, Agent Plugin, and `.mcp/server.json` (including package entries) to match, and release tags to match when supplied. | [Testing and release operations](operations/testing-and-release.md) |

## Safe change routes

- **A score, medal, preference, or suppression is wrong:** trace the representation probe through `evaluation/policies/`, `core/characteristic_morphism.rs`, and `core/omega.rs`; then update the CLI and MCP contract tests that expose the result. Do not repair a presentation symptom before locating the policy/evidence boundary.
- **A source language or graph edge is wrong:** start at `graphs/{ast,uast,cfg,cpg,pdg}/`, preserving UAST node identity and source-span assumptions across graph families. Use the UAST decision record for a normalization change rather than treating native parser output as the cross-language contract.
- **COMPOSABLE is missing, stale, or inconsistent:** inspect GitNexus availability, selected repository root, store freshness, and MDG ingestion before changing the quality policy. Reproduce with the CI fixture when changing the adapter or gate.
- **An agent cannot call a tool or sees incompatible MCP behavior:** follow `topos-mcp` `main.rs` → `server.rs` → the router module and its schema/formatter. Exercise both the initialize and `server/discover` paths, not only an in-process handler test.
- **An MCP path is rejected or unexpectedly accepted:** begin with `resolve_project_path` and test configured-root, project-marker, absolute-path, and existing-symlink cases. For a graph-store override whose leaf is missing, test `resolve_existing_prefix` separately. File containment is an authorization boundary, not an output-format concern.
- **A harness install leaves residue or breaks another tool:** change the installer table/format module and extend the real-binary scratch-home E2E suite. Preserve fields and files Topos does not own.
- **A VS Code runtime issue occurs:** distinguish provider/API availability from binary resolution. Test the relevant ordered fallback and checksum/download failure path in `runtime.ts`, then run the extension package checks.
- **The shell installer sees another Topos binary:** start with `preflight_existing_installs` in `install.sh`, not the Rust harness installer. Exercise the same-path, foreign Homebrew, forced, and piped-stdin cases with `pytest tests/packaging/test_install_sh_preflight.py`.
- **A release artifact or registry version diverges:** change root workspace metadata first, run `python3 scripts/check_versions.py`, and inspect the release matrix and extension packaging path before changing an installer or registry manifest.

## Operational invariants

- `topos-engine` (`topos/engine`) is shared analysis code and must not absorb CLI, MCP transport, or Python-binding responsibilities.
- The command `topos mcp` and the Agent Plugin registration are the common bridge from installed CLI to stdio MCP; the standalone `topos-mcp` binary exists for direct MCP clients and the bin wheel.
- Native Rust tests are primarily module-local, while the installer E2E and MCP lifecycle suites cover boundaries that unit tests cannot: real filesystem mutation and wire-level stdio behavior.
- CI needs the prebuilt `lbug` setup (`scripts/setup-lbug-prebuilt.sh`) before Cargo caching/build steps that link the engine; retain that ordering when changing workflow caching or analysis dependencies.
