---
name: topos
description: Structural code quality metrics, lattice verification, and refactor loops for agent-written code.
version: 0.3.12
homepage: https://docs.krv.ai/topos/
metadata:
  openclaw:
    requires:
      bins: [topos]
    homepage: https://docs.krv.ai/topos/
    os: [macos, linux]
    emoji: "📐"
  hermes:
    tags: [code-quality, refactoring, security, metrics]
    category: software-development
    requires_toolsets: [terminal]
---

# Topos

Topos scores code on three pillars — **SIMPLE**, **COMPOSABLE**, **SECURE** — and maps results to a medal lattice (SLOP → GOLD). Use it in a closed loop: measure, edit, re-measure.

## Use Case

Developers and AI coding agents use this skill to improve structural code quality, reduce complexity, verify refactors, and optimize toward GOLD or SILVER medals. It supports both CLI and MCP agent loops on local repositories.

**Deployment geography:** Global (local execution; no region-restricted services).

## When to Use

Load this skill when the user asks to improve code quality, reduce complexity, check structural security footguns, verify a refactor, or optimize toward GOLD/SILVER medals.

## Requirements / Dependencies

**Requires API Key or External Credential:** No

**Credential Type(s):** None

**Runtime dependencies:**

- `topos` CLI on `PATH` (install via [docs.krv.ai/topos/install.sh](https://docs.krv.ai/topos/install.sh))
- Git repository for baseline comparisons (`topos assess_worktree_change`, untracked baselines via snapshot flow)
- `.gitnexus` dependency graph for COMPOSABLE / GOLD scoring (`topos depgraph generate` or `topos_generate_depgraph`)

**Optional MCP setup** (for tool-based agents, not required for CLI-only use):

```bash
claude mcp add --transport stdio topos -- topos mcp
```

Do not include secrets in prompts, logs, or output. Topos reads local source files and git state only; it does not transmit code to external services.

## Known Risks and Mitigations

Risk: The skill may guide agents to apply structural refactors that change behavior; Topos measures structure, not functional correctness.

Mitigation: Run project tests or linters after each edit; treat Topos verdicts as structural signals, not proof of correctness.

Risk: Agents may trust SECURE medal findings as full security assurance; Topos SECURE checks are structural heuristics, not full SAST.

Mitigation: Pair with dedicated security tooling for high-stakes code; acknowledge remaining SECURE findings explicitly.

Risk: Without a GitNexus graph, COMPOSABLE scores are unavailable or misleading.

Mitigation: Run `topos depgraph generate` (or `topos_generate_depgraph`) before trusting composability scores.

Risk: Cosmetic edits (whitespace, rename-only) may appear as improvements but do not move the lattice.

Mitigation: Stop when MCP returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE`; require `IMPROVEMENT` or `IMPROVEMENT_SCORE` before accepting a change.

## Skill Output

**Output type(s):** Analysis, markdown reports, JSON (MCP), shell commands

**Output format:** CLI tables and ranked file lists; MCP structured payloads with `agent_contract` fields; per-function inspect detail

**Output parameters:** Medal verdict (SLOP → GOLD), pillar scores (SIMPLE, COMPOSABLE, SECURE), ranked refactor targets, assessment status (`IMPROVEMENT`, `REGRESSION`, etc.)

**Other properties:** Writes `.gitnexus` graph artifacts when depgraph is generated; does not modify source files unless the agent chooses to edit based on guidance

## References

- [Topos documentation](https://docs.krv.ai/topos/)
- [Agent contract](https://docs.krv.ai/topos/agents.html)
- [Source repository](https://github.com/Krv-Labs/topos)
- [ClawHub listing](https://clawhub.ai/Krv-Labs/topos)

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

## Ethical Considerations

Users should review agent-proposed code changes before committing, especially when refactoring production systems. Topos is an advisory structural quality tool; organizations should apply their own security, compliance, and code-review policies before deployment.
