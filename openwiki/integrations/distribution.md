---
type: Integration Guide
title: Analysis integrations and distribution surfaces
description: Documents Topos dependencies on GitNexus plus embedded Sighthound, MCP, Docker, VS Code, package metadata, and runtime trust boundaries.
resource: /Dockerfile
tags: [integrations, gitnexus, sighthound, mcp, docker, vscode, rust]
---

# Analysis integrations and distribution surfaces

Topos unifies structural signals into one three-pillar verdict. GitNexus supplies inter-module topology, while the Rust MCP package embeds Sighthound for supplementary security findings. These integrations feed the [quality model](../domain/quality-model.md) through the [architecture pipeline](../architecture/overview.md).

## GitNexus for COMPOSABLE

GitNexus generates `.gitnexus/`, containing a LadybugDB-backed knowledge graph. `ModuleDependencyGraph` parses nodes and typed relationships such as `IMPORTS`, `CALLS`, and `INHERITS`, then derives coupling, instability, fan-in/out, and dependency-depth metrics.

```bash
npm install -g gitnexus@1.6.8
topos depgraph generate
# Underlying generation: gitnexus analyze --skip-agents-md
```

Missing, stale, invalid-path, branch-mismatched, or schema-mismatched stores degrade or block COMPOSABLE according to status rather than crashing an evaluation. A schema mismatch is not silently regenerated. The CLI and MCP status/generation route is explained in [agent workflow guidance](../workflows/agent-and-cli.md#mcp-agent-loop).

## Embedded Sighthound for SECURE

`topos-mcp` depends on a pinned Sighthound crate, so the server/container compile it into the Rust distribution rather than invoking a user-installed `sighthound` executable. Native CPG probes remain Topos’s local structural SECURE mechanism; Sighthound contributes supplementary finding handling. Changes to that boundary belong in `topos/mcp/src/{security,security_findings,sighthound}.rs` and CPG/SECURE tests together.

## MCP package and registry

`topos mcp` launches the in-process Rust server; the `topos-mcp` binary starts that same server directly for MCP clients. `.mcp/server.json` declares registry identity, package metadata, version, and stdio transport. Keep it aligned with `Cargo.toml` and VS Code metadata using `scripts/check_versions.py`.

## Container and editor surfaces

### Docker / Glama

The Dockerfile builds a Maturin `bin` wheel for the compiled `topos-mcp` server, then installs it in a runtime image with Node.js, Git, and pinned GitNexus. It sets `TOPOS_MCP_FILE_ROOT=/workspace` and uses `topos-mcp` as its stdio entrypoint. Mount source below that trusted root or deliberately configure another root.

### VS Code extension

`extensions/vscode/` contributes an MCP server provider plus project-evaluation and dependency-graph commands. It launches `topos mcp` with the workspace as `TOPOS_MCP_FILE_ROOT`, resolves an executable from configured/bundled/cached/PATH sources, and packages platform-specific VSIX artifacts in release CI.

## Change checklist

- GitNexus loader/metric changes: `topos/engine/src/{adapters/gitnexus.rs,graphs/mdg/}` and the composable CI fixture.
- Security changes: exercise native CPG behavior and embedded Sighthound result handling together.
- MCP/container/editor changes: verify trusted-root behavior, stdio entry points, metadata parity, and focused extension or wheel checks.
- Do not expose credentials or document secret values; workflow secret identifiers are sufficient for operations.
