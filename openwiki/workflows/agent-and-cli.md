---
type: Workflow Guide
title: CLI, MCP, and agent improvement workflows
description: Practical guide to Topos Rust CLI commands, stdio MCP tool loops, dependency-graph setup, agent-harness registration, and advisory graph analysis.
resource: /topos/cli/src/main.rs
tags: [workflows, cli, mcp, agents, refactoring, rust]
openwiki:
  roles: [workflow, integration]
  change_kinds: [cli, mcp, evaluation, gitnexus]
  source_paths: [topos/cli/src/main.rs, topos/cli/src/commands, topos/mcp/src/server.rs]
  symbols: [Command, ToposServer, ToolRouter]
  test_paths: [topos/cli/src/main.rs]
  validation_commands: [cargo test -p topos, cargo test -p topos-mcp]
---

# CLI, MCP, and agent improvement workflows

The Clap CLI and RMCP stdio server are two interfaces over the shared [analysis engine](../architecture/overview.md). Use the CLI for direct evaluation and lifecycle management of local agent registrations; use MCP for an agent’s evaluate-edit-assess loop. Both report the [three-pillar quality model](../domain/quality-model.md).

## CLI commands

| Command | Use |
| --- | --- |
| `topos evaluate PATH [-r]` | Evaluate file(s) and aggregate directory/project results; discovers supported languages unless filtered with `--language` |
| `topos inspect FILE` | Inspect detailed metrics for one file |
| `topos compare SOURCE TARGET` | Compare structural AST distance |
| `topos coverage` | Report structural UAST test coverage, outside the medal lattice |
| `topos graphify …` | Generate Graphify knowledge graphs or find orphans |
| `topos depgraph generate [PATH]` | Prepare or refresh GitNexus topology |
| `topos install [HARNESS…]`, `topos uninstall [HARNESS…]`, `topos status` | Register, remove, or inspect Topos-owned MCP entries; see [registration lifecycle](harness-registration.md) |
| `topos mcp` | Start the Rust stdio MCP server |

The root command dispatch is `Command` in `topos/cli/src/main.rs`. `evaluate` supports recursive discovery, JSON, `--language`, priority-based remediation guidance, GitNexus directory selection, and security acknowledgements. `--priority` accepts either one pillar or a full comma-separated `simple,composable,secure` ranking: a single pillar moves that pillar to the front of the existing project ranking, while a full ranking replaces it. `topos config set --priority …` persists the resolved ranking as `[evaluation].priority`; the legacy `preferences` array remains load-compatible but is removed on the next config write. Priority changes refactor ordering and output metadata, not fixed gates in the [quality model](../domain/quality-model.md).

## Baseline evaluation loop

```bash
# SIMPLE and SECURE do not require a dependency store
topos evaluate src/ -r

# Build or refresh cross-module state for COMPOSABLE
npm install -g gitnexus@1.6.8
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus
```

`evaluate` and `inspect` resolve or generate GitNexus state unless `--no-composable` is set. A `--gitnexus-dir` override identifies the store **and its parent is the COMPOSABLE project root**; it is not merely a store selector. Resolve a relative override once before passing it through status, generation, or evaluation paths—rejoining it after root derivation would point to a doubled path. If the requested in-root store does not yet exist, Topos may generate it; an outside-root or schema-mismatched store is not silently accepted or regenerated.

If GitNexus is unavailable, generation fails, or the store cannot be loaded, evaluation continues with SIMPLE and SECURE while COMPOSABLE is reported as unmeasured rather than failed. Regenerate after import/module/directory changes and relevant working-tree edits; COMPOSABLE must not silently score stale topology. The loader, freshness, and distribution constraints are canonical in the [GitNexus integration](../integrations/distribution.md#gitnexus-for-composable).

## MCP agent loop

`topos mcp` launches the in-process `topos-mcp` RMCP server. MCP clients may also run the `topos-mcp` binary directly; both routes link the same engine. `ToposServer::new` in `topos/mcp/src/server.rs` combines the ten tool routers. It exposes tools, `topos://docs/*` resources, the `topos://build` resource, and the `topos_refactor_until_ideal` prompt.

The normal agent loop is: evaluate the target, make one focused edit, then use the assessment/snapshot tool appropriate to whether a Git baseline exists. Before requesting COMPOSABLE or an all-pillar verdict, inspect dependency-graph status and generate only when needed. Generation changes local `.gitnexus` state; status is read-only.

Tool schemas and descriptions have a context budget. Keep surface changes deliberate and validate the MCP stdio `initialize`/`tools/list` smoke route in [testing operations](../operations/testing-and-release.md). The server’s file access is constrained by `TOPOS_MCP_FILE_ROOT` or auto-detected project markers; its symlink-aware containment invariant belongs to [distribution surfaces](../integrations/distribution.md#mcp-file-access-boundary).

## Advisory analysis

Graphify and refactor findings are separate from scored remediation. They identify structural hotspots or orphans but do not feed SIMPLE, COMPOSABLE, or SECURE. Preserve that line between advisory output and the [quality model](../domain/quality-model.md#scoring-versus-advice) when adding tools. Graphify input handling should prefer array-valued `links`, then array-valued `edges`, and reject an oversized `graph.json` before parsing; do not treat malformed/non-array values as graph edges.

## Change navigation

- Add a human command at `Command` in `topos/cli/src/main.rs`, wire its `commands/` module, and add a direct CLI regression or smoke case.
- Add an MCP tool in `topos/mcp/src/tools/`, include its router in `ToposServer::new`, then confirm `tools/list` exposes the consumer-facing schema; an internal tool test alone is insufficient.
- Change harness registration only through the [registration lifecycle](harness-registration.md); it is a user-home mutation boundary, not generic CLI configuration.
- Change GitNexus root/status behavior across CLI and MCP routes together, then test both an available store and graceful degradation.
- Run `cargo test -p topos` for CLI-only changes or `cargo test -p topos-mcp` for server-only changes. Run workspace checks only when shared engine behavior or a cross-crate surface changes.
