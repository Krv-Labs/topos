---
type: Workflow Guide
title: CLI, MCP, and agent improvement workflows
description: Practical guide to Topos terminal commands, MCP tool loops, dependency-graph setup, snapshots, and separate advisory refactoring workflows.
resource: /topos/cli/main.py
tags: [workflows, cli, mcp, agents, refactoring]
---

# CLI, MCP, and agent improvement workflows

Topos exposes the same quality system through Click commands and an stdio FastMCP server. The CLI is the direct human interface; MCP is designed for coding agents that evaluate, edit, and re-assess their work. Both depend on the [quality model](../domain/quality-model.md) and invoke the [hybrid analysis pipeline](../architecture/overview.md).

## CLI commands

The root Click group lazily registers commands while preserving a lightweight `--version` / root-help fast path. Main commands are:

| Command | Use |
| --- | --- |
| `topos evaluate PATH [-r]` | Evaluate file(s) and aggregate a directory/project result |
| `topos inspect FILE` | Inspect detailed metrics for one file |
| `topos compare SOURCE TARGET` | Compare structural AST distance |
| `topos coverage` | Report structural UAST test coverage, outside the medal lattice |
| `topos depgraph generate` | Invoke GitNexus to prepare `.gitnexus/` |
| `topos refactor cycles|dependencies|process` | Produce advisory structural hotspots |
| `topos mcp` | Start stdio MCP server |
| `topos update` / `topos uninstall` | Maintain an installed distribution |

`evaluate` supports recursive discovery, JSON, verbosity, `--language`, priority/preferences, a GitNexus directory, and ephemeral security acknowledgements. Its `--preferences` first value becomes output priority only; it does not loosen thresholds.

## Baseline evaluation loop

```bash
# SIMPLE and SECURE are available without a dependency store
topos evaluate src/ -r --language python

# Build or refresh cross-module state for COMPOSABLE
npm install -g gitnexus
topos depgraph generate

# Include it explicitly in evaluation
topos evaluate src/ -r --gitnexus-dir .gitnexus
```

GitNexus generation runs `gitnexus analyze --skip-agents-md`. Re-run it after import/module/directory changes and after relevant working-tree edits: freshness logic intentionally detects source modification so an edit-assess loop should not score against stale topology. See [integration details](../integrations/distribution.md#gitnexus-for-composable).

## MCP agent loop

`topos mcp` starts FastMCP as `topos_mcp` over stdio. Registration happens at server startup by importing its tool, resource, and prompt packages. The server instructions define the normal loop:

1. Call `topos_evaluate_file`.
2. Edit the target file in place.
3. Call `topos_assess_worktree_change` to compare against the Git baseline.
4. For uncommitted/untracked baselines, call `topos_begin_refactor` first and then `topos_assess_snapshot`.
5. Use `topos_assess_improvement` only for side-by-side source variants.

Before seeking COMPOSABLE or IDEAL, call `topos_depgraph_status`; call `topos_generate_depgraph` only when state/routing says it is needed. Generation is side-effecting and approval-gated; status is read-only. The compact `topos://docs/agent-contract` resource and `topos_get_doc(topic="agent-contract")` provide the authoritative per-tool routing contract.

The MCP result layer adds source locations, suggestions, security findings, preferences, project aggregation, assessments, comparisons, coverage, and refactor results around the same evaluation engine. Contract output size is intentionally guarded by `tests/mcp/test_context_budget.py`; avoid casually expanding tool schemas or descriptions.

## Advisory refactoring

`topos refactor` and MCP `topos_refactor(target=...)` are intentionally separate from verdict remediation:

- `cycles`: extracts a fundamental CFG cycle basis and maps cycles to source ranges.
- `dependencies`: ranks GitNexus MDG dependency edges by balanced Forman curvature.
- `process`: ranks GitNexus process transitions by directed Forman-Ricci curvature.

These tools are read-only advisory analysis. In contrast, evaluation `refactor_targets` identify failed gates inside the scoring model. The cycle-basis and curvature methods span the [hybrid architecture](../architecture/overview.md#native-code-boundary) and must remain outside the quality-policy inputs; keep that distinction intact when adding tool output.

## Structural test coverage

`topos coverage` is a static UAST-overlap measure for a program-under-test (PUT) and test files; it is **not** executed line/branch coverage and does not prove that a test invokes a production declaration. It extracts `FunctionDecl` and `MethodDecl` nodes, fingerprints each declaration body by its UAST-kind multiset, and assigns each PUT declaration its best test-declaration match by recall. The report includes mean declaration coverage, statement/expression recall, an F2 signal that penalizes structurally unrelated test mass, and diagnostics for declarations below the chosen threshold.

The canonical product explanation and command reference remain in [Sphinx Measures](../../docs/source/measures.rst). The implementation is `topos/functors/profunctors/uast/structural_test_coverage.py`; parser/UAST changes therefore require coverage-focused tests as well as the broader [analysis architecture](../architecture/overview.md). Treat `Unknown` kinds and framework/mock-heavy tests as interpretation hazards, particularly when `--include-unknown` is enabled.

## Interface and behavior cautions

- CLI command registration avoids importing the heavy evaluation/Numpy stack for trivial root invocations. Preserve this when adding root behavior.
- MCP file access is expected to be rooted through `TOPOS_MCP_FILE_ROOT`, especially in the container and VS Code host. Refer to [distribution surfaces](../integrations/distribution.md#container-and-editor-surfaces) before changing filesystem behavior.
- `topos coverage` is a separate structural signal; do not treat coverage changes as a medal-policy change.
- Security `--allow` and `.topos.toml` acknowledgements are visible and grade-capped, not a mechanism to erase the raw SECURE finding.
