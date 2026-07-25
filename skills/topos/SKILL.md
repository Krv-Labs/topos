---
name: topos
description: Structural code quality metrics, lattice verification, and refactor loops for agent-written code.
version: "0.4.1"
homepage: https://docs.krv.ai/topos/
metadata:
  openclaw:
    requires:
      bins: [topos]
    homepage: https://docs.krv.ai/topos/
  hermes:
    tags: [code-quality, refactoring, security, metrics]
    category: software-development
    requires_toolsets: [terminal]
---

# Topos

Topos scores code on three pillars — **SIMPLE**, **COMPOSABLE**, **SECURE** — and maps results to a medal lattice (SLOP → GOLD). Use it in a closed loop: measure, edit, re-measure.

## When to Use

Load this skill when the user asks to improve code quality, reduce complexity, check structural security footguns, verify a refactor, or optimize toward GOLD/SILVER medals.

## Prerequisites

```bash
curl -fsSL https://docs.krv.ai/topos/install.sh | sh
npm install -g gitnexus   # enables COMPOSABLE / GOLD scoring
```

COMPOSABLE is scored by default: `evaluate` / `inspect` and the MCP evaluate
tools detect a missing or stale `.gitnexus` and regenerate it before scoring.
Run `topos depgraph generate` only to force a refresh.

For MCP-based agents, register the server:

```bash
claude mcp add --transport stdio topos -- topos mcp
```

## Agent Loop

1. **Measure** — `topos evaluate <path> -r` (CLI) or `topos_evaluate_file` / `topos_evaluate_project` (MCP). COMPOSABLE is included by default; pass `gitnexus_dir` only to point at a non-default graph.
2. **Inspect** — `topos inspect <file>` or `topos_inspect_code` for per-function complexity and metric detail.
3. **Edit** — one focused structural change (extract helper, simplify branch, decouple import).
4. **Verify** — re-run evaluate, or use `topos_assess_worktree_change` (baseline `HEAD`) for MCP loops. For untracked baselines: `topos_begin_refactor` → edit → `topos_assess_snapshot`.
5. **Behavior check** — run project tests or linters; Topos does not prove correctness.

Stop when the target medal is reached, the priority pillar passes, or further iterations plateau. Prefer structured `agent_contract` fields over parsing prose.

## CLI Reference

| Command | Purpose |
| --- | --- |
| `topos evaluate <path> -r` | Rank files; show worst offenders and cheapest fixes |
| `topos inspect <file>` | Deep per-file metrics and suggestions |
| `topos compare <a> <b>` | AST edit distance between two versions |
| `topos coverage <put>... --tests <test>` | Structural test coverage (UAST + k-gram recall) |
| `topos depgraph generate` | Build GitNexus graph for COMPOSABLE scoring |
| `topos graphify generate\|orphans` | Advisory orphan / fragile-edge hints (does not affect evaluate) |
| `topos mcp` | Start the MCP server for tool-based agent loops |

Pass `--gitnexus-dir .gitnexus` when the graph lives outside the default path, or `--no-composable` to score SIMPLE/SECURE only. Preference ranking is an MCP-only input (`preferences.ranking`); the CLI has no `--preferences` flag. Advisory `cycles`/`dependencies`/`process` hints are likewise MCP-only, via `topos_refactor`.

## MCP Tool Reference

| Tool | Purpose |
| --- | --- |
| `topos_get_doc(topic="agent-contract")` | Compact loop contract — read first |
| `topos_evaluate_file` | Score one file; returns 3 ranked edit spans (`refactor_targets`, gate failures first) |
| `topos_evaluate_project` | Project rollup and worst-file list |
| `topos_inspect_code` | Deep per-function complexity and metrics |
| `topos_assess_worktree_change` | Compare working tree to a git baseline |
| `topos_begin_refactor` / `topos_assess_snapshot` | Snapshot flow for untracked baselines |
| `topos_assess_improvement` | Side-by-side variant comparison |
| `topos_assess_changeset` | Assess several edited files at once against a git baseline |
| `topos_generate_depgraph` / `topos_depgraph_status` | Force-refresh, or read-only diagnose, the GitNexus graph |
| `topos_calculate_coverage` | Structural test coverage (separate from lattice) |
| `topos_evaluate_code` | Score a source string when there is no file on disk |
| `topos_inspect_code` / `topos_compare_code` / `topos_compare_files` | Deep metrics; AST edit distance between two versions |
| `topos_preference_walk` | Resolve target / fallback / next-step verdicts for a ranking |
| `topos_refactor` | Advisory hotspots (`cycles`, `dependencies`, `process`, `graphify`) — never affects the medal |
| `topos_generate_graphify_graph` | Build the Graphify knowledge graph for `topos_refactor(target="graphify")` |

MCP tool arguments are **flat objects** — `{"filepath": "..."}`, not `{"params": {...}}`.

## Pitfalls

- **No GitNexus → no COMPOSABLE.** The graph is generated automatically, but only if `gitnexus` is installed. If it isn't, `coupling_available` is `false` and GOLD is unreachable — check `warnings`.
- **Cosmetic edits don't count.** Whitespace and rename-only changes won't move the lattice; MCP returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.
- **SECURE is structural, not full SAST.** Pair with dedicated security tooling for high-stakes code.
- **`topos refactor` is advisory.** It does not replace `topos evaluate` for scoring.

## Verification

A change is ready when:

- Assessment status is `IMPROVEMENT` or `IMPROVEMENT_SCORE` (MCP), or the evaluate verdict improved (CLI).
- Status is not `SUSPICIOUS_NO_STRUCTURAL_CHANGE` or `REGRESSION`.
- Active SECURE findings are fixed or explicitly acknowledged.
- Relevant tests/type checks pass, or their absence is reported.

Full agent contract: [docs.krv.ai/topos/agents](https://docs.krv.ai/topos/agents.html)
