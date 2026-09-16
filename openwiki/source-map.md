---
type: maintenance source map
title: Topos maintenance source map
description: Ownership-oriented starting points for changing Topos analysis, command-line and MCP delivery, integrations, and release controls. Each route identifies the behavior page, implementation boundary, and focused regression coverage to consult before editing.
resource: /topos
tags: [source-map, maintenance, navigation, rust]
openwiki:
  roles: [repository, testing]
  change_kinds: [navigation]
  source_paths: [topos, extensions/vscode, .github/workflows]
verified:
  - by: openwiki/0.5.0
    at: 2026-09-01T16:34:02.929Z
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
  - id: openwiki-source-9bf1d8e64277056e6ccedf90
    resource: repo://topos/mcp/src/main.rs
  - id: openwiki-source-416dcb63c9e3e0c0c2cb0eed
    resource: repo://topos/mcp/src/security.rs
  - id: openwiki-source-3812b1def9fbad0607404761
    resource: repo://topos/mcp/src/server.rs
  - id: openwiki-source-8680de586193e5fad2de692f
    resource: repo://topos/mcp/tests/lifecycle.rs
generated: { by: "openwiki/0.5.0", at: "2026-09-01T16:34:02.929Z" }
---

# Topos maintenance source map

Use this as a change-planning map, not as an inventory. Start with the system row that owns the observable behavior, read its linked wiki page for the contract, then follow the entrypoint through the shared engine or integration boundary. The workspace has three Rust crates: `topos-engine` is the transport-free analysis library; `topos` owns the human CLI and invokes the MCP server in-process; `topos-mcp` owns stdio protocol delivery. `Cargo.toml` is the workspace/version authority.

## Owned systems

| Change concern | Ownership and starting points | Focused checks and decisions | Behavior reference |
| --- | --- | --- | --- |
| **Quality model and verdicts** | `topos/engine/src/core/` defines the categorical primitives, four-generator `Omega`, and characteristic morphism. `topos/engine/src/evaluation/` translates representation evidence into policies, preferences, suggestions, and suppression. Change this layer before changing CLI or MCP rendering. | Keep the four pillars (SIMPLE, COMPOSABLE, SECURE, NAVIGABLE) coherent across policy, preference, and output. `topos/engine/src/config.rs` loads the nearest `.topos.toml`; malformed or absent configuration falls back to an empty config, while acknowledged security patterns require a non-empty reason and remain disclosed rather than silently improving the canonical verdict. | [Quality model](domain/quality-model.md) |
| **Parsing and structural representations** | `topos/engine/src/graphs/` owns AST, UAST, CFG, CPG, PDG, MDG, and process representations; `graphs/base.rs` is the shared representation protocol. `topos/engine/src/functors/` owns probes and cross-representation profunctors. | Put a parser/normalization regression beside its responsible graph or probe module. For cross-language UAST shape and provenance decisions, consult `docs/decisions/uast-industry-standards.md`; CFG control-flow semantics have adjacent module tests. | [Architecture overview](architecture/overview.md) |
| **Dependency topology and COMPOSABLE** | `topos/engine/src/adapters/gitnexus.rs` owns GitNexus invocation/store handling; `graphs/mdg/` interprets module dependency data. The CLI `commands/depgraph/` and MCP `tools/depgraph.rs` expose build/status operations. | COMPOSABLE is git-root scoped: do not derive the `.gitnexus` store from a nested Cargo package. The CI `composable` job installs pinned GitNexus, analyzes a fixture, and asserts that `evaluate` emits a non-null COMPOSABLE score rather than silently taking the missing-store path. The file-level gate measures outward dependency burden; detailed topology remains advisory. | [Quality model](domain/quality-model.md) and [Integrations and distribution](integrations/distribution.md) |
| **CLI commands and presentation** | `topos/cli/src/main.rs` is the Clap dispatch boundary. `commands/evaluate/` and `commands/inspect/` adapt engine outcomes for users; `compare.rs`, `coverage.rs`, and `composable.rs` provide advisory structural workflows. `commands/mcp.rs` starts `topos_mcp::server::serve` on a Tokio runtime. | Preserve the distinction between lattice evaluation and advisory comparison/coverage. A bare CLI exits with help; command errors are printed and exit non-zero. Add unit coverage near the command for parsing, output, and error behavior. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| **Harness registration** | `topos/cli/src/commands/install/` owns the harness table, config-format adapters, absolute-binary resolution, atomic filesystem work, state inspection, and uninstall cleanup. | Installation writes only the Topos MCP entry, not shared instructions or skills. Non-interactive install needs explicit harnesses or `--all`; destructive uninstall differentiates interactive, headless, and ambiguous stream arrangements. `topos/cli/tests/install_e2e.rs` drives the real binary against a scratch `$HOME` to protect round-trip cleanup, foreign configuration preservation, exit behavior, and JSON reporting. `docs/decisions/cli-harness-install.md` is the compatibility and format decision record. | [Agent-harness registration](workflows/harness-registration.md) |
| **MCP protocol, agent tools, and state** | `topos/mcp/src/main.rs` is the standalone stdio binary; `server.rs` combines the routers in `tools/` and directly serves static docs/resources and the refactor prompt. Tool modules own evaluation, assessment/snapshots, comparison, coverage, dependency graphs, inspection, preferences, and refactor targets. `schemas.rs` and `formatting.rs` own the agent-facing wire shape. | Do not add a protocol revision implicitly: `server.rs` pins the negotiated revisions. Keep initialize and stateless discovery on the same tool surface. `topos/mcp/tests/lifecycle.rs` pipes JSON-RPC into the built binary to exercise both lifecycles and each supported initialize revision. `topos://build` is the low-context diagnostic resource for the serving executable, root, and staleness. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
<!-- openwiki: broken internal link [integrations/distribution.md#mcp-file-access-boundary] heading anchor "mcp-file-access-boundary" does not exist in "integrations/distribution.md". Fix the href or restore the target, then delete this comment. -->
| **MCP filesystem and security evidence** | `topos/mcp/src/security.rs` owns path containment and project/root discovery; `security_findings.rs`, `sighthound.rs`, and `diagnostics.rs` normalize security evidence and overlays. | Filesystem tools must resolve an existing path, discover a project marker, and fail closed outside `TOPOS_MCP_FILE_ROOT` when it is configured. Without that variable, callers must provide an absolute path. Preserve incremental symlink resolution for missing leaves so a link cannot escape the boundary. | [Integrations and distribution](integrations/distribution.md#mcp-file-access-boundary) |
| **VS Code integration** | `extensions/vscode/src/extension.ts` registers the MCP definition provider and Command Palette workflows. `runtime.ts` owns invocation construction, runtime discovery/download, manifest selection, checksums, redirects, and timeouts. | The extension passes its workspace as `TOPOS_MCP_FILE_ROOT`, so it participates in the MCP access boundary. Runtime resolution is ordered: configured executable, bundled binary, cache, PATH, optional virtual-environment discovery, then optional verified download. Run `pnpm run check-types`, `pnpm run lint`, and `pnpm run test:unit`; integration tests are separately available. | [Integrations and distribution](integrations/distribution.md) |
| **Agent Plugin and skill package** | `agent-plugin/plugin.json` is package metadata; `agent-plugin/mcp.json` is its stdio registration and invokes `topos mcp`; `agent-plugin/skills/topos/` holds the distributed skill. | Run `scripts/check_agent_plugin.py` and `scripts/check_skill.py` after changing package or skill material. Keep plugin metadata and registration distinct from the CLI installer, which changes local harness configurations. | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| **CI, publication, and versioning** | `.github/workflows/ci.yml` is the admission and verification workflow; `scripts/ci_gate.py` owns the stacked-PR decision. `.github/workflows/release.yml` builds native artifacts, packages VSIX targets, publishes releases, and handles downstream publication. `scripts/check_versions.py` checks published metadata against the workspace version. | The CI gate runs trunk and stacked PRs; unavailable or malformed stack data fails open so verification is not silently skipped. The Rust job runs formatting, warning-denying Clippy, workspace tests, package/skill checks, and a stdio MCP smoke test. Version changes begin in root `Cargo.toml`; `check_versions.py` requires the VS Code package, Agent Plugin, and `.mcp/server.json` (including package entries) to match, and release tags to match when supplied. | [Testing and release operations](operations/testing-and-release.md) |

## Safe change routes

- **A score, medal, preference, or suppression is wrong:** trace the representation probe through `evaluation/policies/`, `core/characteristic_morphism.rs`, and `core/omega.rs`; then update the CLI and MCP contract tests that expose the result. Do not repair a presentation symptom before locating the policy/evidence boundary.
- **A source language or graph edge is wrong:** start at `graphs/{ast,uast,cfg,cpg,pdg}/`, preserving UAST node identity and source-span assumptions across graph families. Use the UAST decision record for a normalization change rather than treating native parser output as the cross-language contract.
- **COMPOSABLE is missing, stale, or inconsistent:** inspect GitNexus availability, selected repository root, store freshness, and MDG ingestion before changing the quality policy. Reproduce with the CI fixture when changing the adapter or gate.
- **An agent cannot call a tool or sees incompatible MCP behavior:** follow `topos-mcp` `main.rs` → `server.rs` → the router module and its schema/formatter. Exercise both the initialize and `server/discover` paths, not only an in-process handler test.
- **An MCP path is rejected or unexpectedly accepted:** begin with `resolve_project_path` and test the configured-root, project-marker, absolute-path, symlink, and missing-path cases. File containment is an authorization boundary, not an output-format concern.
- **A harness install leaves residue or breaks another tool:** change the installer table/format module and extend the real-binary scratch-home E2E suite. Preserve fields and files Topos does not own.
- **A VS Code runtime issue occurs:** distinguish provider/API availability from binary resolution. Test the relevant ordered fallback and checksum/download failure path in `runtime.ts`, then run the extension package checks.
- **A release artifact or registry version diverges:** change root workspace metadata first, run `python3 scripts/check_versions.py`, and inspect the release matrix and extension packaging path before changing an installer or registry manifest.

## Operational invariants

- `topos-engine` is shared analysis code and must not absorb CLI, MCP transport, or Python-binding responsibilities.
- The command `topos mcp` and the Agent Plugin registration are the common bridge from installed CLI to stdio MCP; the standalone `topos-mcp` binary exists for direct MCP clients and the bin wheel.
- Native Rust tests are primarily module-local, while the installer E2E and MCP lifecycle suites cover boundaries that unit tests cannot: real filesystem mutation and wire-level stdio behavior.
- CI needs the prebuilt `lbug` setup before Cargo caching/build steps that link the engine; retain that ordering when changing workflow caching or analysis dependencies.
