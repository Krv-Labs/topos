---
name: topos
description: Structural code quality metrics, lattice verification, and refactor loops for agent-written code.
version: "0.4.0"
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
topos depgraph generate   # required for COMPOSABLE / GOLD scoring
```

For MCP-based agents, register the server:

```bash
claude mcp add --transport stdio topos -- topos mcp
```

## Agent Loop

1. **Measure** — `topos evaluate <path> -r` (CLI) or `topos_evaluate_file` / `topos_evaluate_project` (MCP). Pass `gitnexus_dir` for COMPOSABLE.
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
| `topos coverage <path>` | Structural test coverage (UAST + k-gram recall) |
| `topos depgraph generate` | Build GitNexus graph for COMPOSABLE scoring |
| `topos refactor cycles\|dependencies\|process` | Advisory refactor hints (does not affect evaluate) |
| `topos mcp` | Start the MCP server for tool-based agent loops |

Pass `--gitnexus-dir .gitnexus` when the graph lives outside the default path. Use `--preferences simple,composable,secure` to steer which pillar to protect first.

## MCP Tool Reference

| Tool | Purpose |
| --- | --- |
| `topos_get_doc(topic="agent-contract")` | Compact loop contract — read first |
| `topos_evaluate_file` | Score one file; optional `refactor_targets` for ranked edit spans |
| `topos_evaluate_project` | Project rollup and worst-file list |
| `topos_inspect_code` | Deep per-function complexity and metrics |
| `topos_assess_worktree_change` | Compare working tree to a git baseline |
| `topos_begin_refactor` / `topos_assess_snapshot` | Snapshot flow for untracked baselines |
| `topos_assess_improvement` | Side-by-side variant comparison |
| `topos_generate_depgraph` | Build/refresh GitNexus graph (COMPOSABLE prerequisite) |
| `topos_calculate_coverage` | Structural test coverage (separate from lattice) |

## Pitfalls

- **No graph → no COMPOSABLE.** Run `topos depgraph generate` (or `topos_generate_depgraph`) before trusting composability scores.
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
