---
type: Workflow Guide
title: CLI, MCP, and agent improvement workflows
description: Practical guide to Topos Rust CLI commands, stdio MCP tool loops, dependency-graph setup, and advisory graph analysis.
resource: /topos/cli/src/main.rs
tags: [workflows, cli, mcp, agents, refactoring, rust]
---

# CLI, MCP, and agent improvement workflows

The Clap CLI and RMCP stdio server are two interfaces over the shared [analysis engine](../architecture/overview.md). Use the CLI for direct evaluation; use MCP for an agent’s evaluate-edit-assess loop. Both report the [three-pillar quality model](../domain/quality-model.md).

## CLI commands

| Command | Use |
| --- | --- |
| `topos evaluate PATH [-r]` | Evaluate file(s) and aggregate directory/project results |
| `topos inspect FILE` | Inspect detailed metrics for one file |
| `topos compare SOURCE TARGET` | Compare structural AST distance |
| `topos coverage` | Report structural UAST test coverage, outside the medal lattice |
| `topos graphify …` | Generate Graphify knowledge graphs or find orphans |
| `topos depgraph generate [PATH]` | Prepare or refresh GitNexus topology |
| `topos mcp` | Start the Rust stdio MCP server |

`evaluate` supports recursive discovery, JSON, `--language`, preferences, GitNexus directory selection, and security acknowledgements. The command surface is defined in `topos/cli/src/main.rs`; do not restore removed Python-package self-update/uninstall assumptions without an explicit distribution design.

## Baseline evaluation loop

```bash
# SIMPLE and SECURE do not require a dependency store
topos evaluate src/ -r --language rust

# Build or refresh cross-module state for COMPOSABLE
npm install -g gitnexus@1.6.8
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus
```

`depgraph generate` first checks status, does not regenerate a current store unless `--force` is set, and reports schema mismatch as an error. Regenerate after import/module/directory changes and relevant working-tree edits; COMPOSABLE must not silently score stale topology. See [GitNexus integration details](../integrations/distribution.md#gitnexus-for-composable).

## MCP agent loop

`topos mcp` launches the in-process `topos-mcp` RMCP server. MCP clients may also run the `topos-mcp` binary directly; both routes link the same engine. The server registers tools, resources, and the refactor prompt in `topos/mcp/src/server.rs`.

The normal agent loop is: evaluate the target, make one focused edit, then use the assessment/snapshot tool appropriate to whether a Git baseline exists. Before requesting COMPOSABLE or an all-pillar verdict, inspect dependency-graph status and generate only when needed. Generation changes local `.gitnexus` state; status is read-only.

Tool output has a context budget. Keep schema and description changes deliberate and validate the MCP stdio smoke path in [testing operations](../operations/testing-and-release.md).

## Advisory analysis

Graphify and refactor findings are separate from scored remediation. They identify structural hotspots or orphans but do not feed SIMPLE, COMPOSABLE, or SECURE. Preserve that line between advisory output and the [quality model](../domain/quality-model.md#scoring-versus-advice) when adding tools.

## Interface cautions

- `TOPOS_MCP_FILE_ROOT` bounds MCP file access, including in the container and VS Code host; consult [distribution surfaces](../integrations/distribution.md#container-and-editor-surfaces).
- `topos coverage` is structural UAST overlap, not executed line/branch coverage and not a medal-policy signal.
- Security acknowledgements remain visible and grade-capped rather than deleting raw SECURE evidence.
