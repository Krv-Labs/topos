# COMPOSABLE dependency graphs by default

Status: **SHIPPED since v0.4.0; current in v0.5.0**.

## Decision

CLI and MCP evaluation attempt to resolve or generate a GitNexus dependency
graph before scoring COMPOSABLE. Users can opt out when graph generation is too
expensive or inappropriate.

The shared policy lives in `topos/mcp/src/evaluation/mod.rs`:

- use a valid, fresh graph when present;
- run `gitnexus analyze` when the graph is missing or stale and generation is
  available;
- respect an explicit graph-directory override;
- degrade to "COMPOSABLE not scored" with structured warnings when the graph
  cannot be loaded or generated;
- never fail the other file-quality pillars merely because GitNexus is
  unavailable.

The CLI adds terminal progress through
`topos/cli/src/commands/composable.rs`; MCP captures generation output for its
structured response. The policy is shared even though the presentation differs.

## User controls

- CLI: `--no-composable` skips graph resolution; `--gitnexus-dir <dir>` selects
  a graph store.
- MCP: `no_composable` and `gitnexus_dir` are available on file/project
  evaluation inputs. Inspect exposes the same controls.

Because evaluation may write `.gitnexus/` and launch GitNexus, the relevant MCP
tools are not described as read-only or closed-world operations.

## Failure contract

Graph failures are fail-open for evaluation:

1. SIMPLE, SECURE, and NAVIGABLE still run.
2. COMPOSABLE is omitted rather than guessed.
3. Warnings identify missing tooling, stale data, invalid overrides, schema
   mismatches, branch ownership, or load failures as specifically as possible.

This availability decision is independent of the COMPOSABLE metric policy. In
v0.5.0, a resolved graph supplies the file-level fan-out gate and the richer
advisory/diagnostic readings described in
[`file-level-composable.md`](file-level-composable.md).

## Why this remains the default

COMPOSABLE is one of four independent quality pillars. Requiring a separate
manual graph-generation round trip makes it easy for agents and humans to omit
the pillar unintentionally. Attempting generation by default keeps the common
path complete while the explicit opt-out and fail-open behavior bound its cost
and failure impact.
