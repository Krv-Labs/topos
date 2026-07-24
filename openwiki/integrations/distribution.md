---
type: Integration Guide
title: Analysis integrations and distribution surfaces
description: Documents Topos dependencies on GitNexus and Sighthound plus MCP, Docker, VS Code, package metadata, and runtime trust boundaries.
resource: /Dockerfile
tags: [integrations, gitnexus, sighthound, mcp, docker, vscode]
---

# Analysis integrations and distribution surfaces

Topos is the operator that unifies structural signals into one three-pillar verdict. It delegates specialized work where a separate engine owns better source information: GitNexus supplies inter-module topology and Sighthound can supply SECURE findings. These integrations feed the [quality model](../domain/quality-model.md) through the [architecture pipeline](../architecture/overview.md).

## GitNexus for COMPOSABLE

GitNexus generates `.gitnexus/`, containing a LadybugDB-backed knowledge graph. `ModuleDependencyGraph` parses it into nodes and typed relationships such as `IMPORTS`, `CALLS`, and `INHERITS`, then derives coupling, instability, fan-in/out, and dependency-depth metrics.

```bash
pnpm add -g gitnexus  # or: npm install -g gitnexus
topos depgraph generate
# Equivalent underlying operation: gitnexus analyze --skip-agents-md
```

Operational states matter:

- Missing, stale, invalid-path, branch-mismatch, and schema-mismatch stores degrade COMPOSABLE rather than crashing an evaluation.
- The 0.3.10 freshness fingerprint considers working-tree source edits, preventing an edit-in-place agent loop from reusing topology generated before the edit.
- A Ladybug shadow-page replay failure can trigger a retry with a read-write handle. Treat the store as managed mutable state, not merely an immutable report.
- A schema mismatch requires compatible Topos/GitNexus/Ladybug versions; blindly regenerating may not repair it.

The CLI and MCP status/generation tools expose this setup boundary. Their route is described in [agent workflow guidance](../workflows/agent-and-cli.md#mcp-agent-loop).

## Sighthound for SECURE

`CodePropertyGraph.metrics()` checks for a `sighthound` executable. If present, Topos invokes `sighthound --output-format json` against the real file or a temporary language-suffixed file, normalizes either bare-list or `{findings: [...]}` payloads, and maps search findings to `cpg.dangerous_calls` and taint-tagged findings to `cpg.taint_flows`.

If it is absent or produces no usable JSON, Topos falls back to local CPG danger and taint probes. This optional integration therefore deepens detection without adding a package dependency or making basic SECURE scoring unavailable. Changes to finding tags, schema mapping, or source/sink rendering belong in `topos/utils/sighthound.py`, `topos/mcp/security_findings.py`, and `tests/utils/test_sighthound.py` together.

## MCP package and registry

The project publishes both `topos` and `topos-mcp` console scripts. The latter starts the same stdio server directly; `topos mcp` routes to it via the CLI. `.mcp/server.json` declares the canonical MCP Registry name `io.github.Krv-Labs/topos`, PyPI package (`topos-mcp`), version, and stdio transport. The public GitHub MCP Registry listing and VS Code’s `@mcp topos` discovery flow surface the server used by the [agent-facing MCP workflow](../workflows/agent-and-cli.md#mcp-agent-loop); ClawHub distributes a separate agent skill. Keep registry metadata version-aligned with `Cargo.toml`, Python metadata, and the VS Code package; `scripts/check_versions.py` enforces the contract.

## Container and editor surfaces

### Docker / Glama

The Dockerfile is a two-stage Maturin build: a Python/Rust builder produces a wheel; the runtime installs that wheel plus Node.js, Git, and GitNexus. It defaults `TOPOS_MCP_FILE_ROOT=/workspace`, uses `/workspace` as working directory, and enters through `topos-mcp` over stdio. Mount the repository there or deliberately override the trusted root.

### VS Code extension

`extensions/vscode/` contributes an MCP server provider and two commands: **Evaluate Project** and **Generate Dependency Graph**. It launches `topos mcp` with the workspace as `TOPOS_MCP_FILE_ROOT`; its runtime lookup prefers an explicit executable path, then bundled/cached/PATH/Python installations, then an optional verified download. The extension owns TypeScript build/test code and packages platform-specific VSIX artifacts in release CI.

The extension setting text currently lists five auto-detected languages while core CLI support includes Go. Treat language UI consistency as a follow-up when changing language support.

## Change checklist

- GitNexus loader/metric changes: `topos/graphs/mdg/`, `topos/utils/gitnexus.py`, tests in `tests/utils/`, `tests/mcp/`, and `tests/graphs/mdg/`.
- Security engine changes: exercise Sighthound-present and absent behavior and keep the raw/acknowledged finding distinction stable.
- MCP metadata/container/editor changes: verify `TOPOS_MCP_FILE_ROOT` behavior, stdio entry points, version parity, and focused packaging/extension tests.
- Do not expose credentials or update workflow secrets in documentation; automation references secret names only.
