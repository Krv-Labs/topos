---
name: topos
description: Evaluate and improve code with Topos. Use for complexity reduction, security checks, refactor verification, and PLATINUM/GOLD goals.
version: "0.5.0"
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

Topos scores code on four pillars — **SIMPLE**, **COMPOSABLE**, **SECURE**, **NAVIGABLE** — and maps results to a medal lattice (SLOP → PLATINUM). Use it in a closed loop: measure, edit, re-measure.

## Use Case

Developers and AI coding agents use this skill to improve structural code quality, reduce complexity, verify refactors, and optimize toward PLATINUM or GOLD medals. It supports both CLI and MCP agent loops on local repositories.

**Deployment geography:** Global (local execution; no region-restricted services).

## When to Use

Load this skill when the user asks to improve code quality, reduce complexity, check structural security footguns, verify a refactor, or optimize toward PLATINUM/GOLD medals.

## Requirements / Dependencies

**Requires API Key or External Credential:** No

**Credential Type(s):** None

**Runtime dependencies:**

```bash
curl -fsSL https://docs.krv.ai/topos/install.sh | bash
npm install -g gitnexus   # enables COMPOSABLE / PLATINUM scoring
```

- `topos` CLI on `PATH` (install via [docs.krv.ai/topos/install.sh](https://docs.krv.ai/topos/install.sh))
- Git repository for MCP baseline comparisons (`topos_assess_worktree_change`; untracked baselines via `topos_begin_refactor` → `topos_assess_snapshot`)
- `.gitnexus` dependency graph for COMPOSABLE / PLATINUM scoring (generated automatically when `gitnexus` is installed; force refresh with `topos depgraph generate` or `topos_generate_depgraph`)

COMPOSABLE is scored by default: `evaluate` / `inspect` and the MCP evaluate
tools detect a missing or stale `.gitnexus` and regenerate it before scoring.
Run `topos depgraph generate` only to force a refresh.

**Optional MCP setup** (for tool-based agents, not required for CLI-only use):

```bash
topos install --all   # registers the MCP server in every supported harness
topos status          # verify registration
```

Do not include secrets in prompts, logs, or output. Topos reads local source files and git state only; it does not transmit code to external services.

## Known Risks and Mitigations

Risk: The skill may guide agents to apply structural refactors that change behavior; Topos measures structure, not functional correctness.

Mitigation: Run project tests or linters after each edit; treat Topos verdicts as structural signals, not proof of correctness.

Risk: Agents may trust SECURE medal findings as full security assurance; Topos SECURE checks are structural heuristics, not full SAST.

Mitigation: Pair with dedicated security tooling for high-stakes code; acknowledge remaining SECURE findings explicitly.

Risk: Without GitNexus installed, COMPOSABLE scores are unavailable and PLATINUM is unreachable.

Mitigation: Install `gitnexus` (`npm install -g gitnexus`); check MCP `warnings` and `coupling_available` before trusting composability scores.

Risk: Cosmetic edits (whitespace, rename-only) may appear as improvements but do not move the lattice.

Mitigation: Stop when MCP returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE`; require `IMPROVEMENT` or `IMPROVEMENT_SCORE` before accepting a change.

## Skill Output

**Output type(s):** Analysis, markdown reports, JSON (MCP), shell commands

**Output format:** CLI tables and ranked file lists; MCP structured payloads with `agent_contract` fields; per-function inspect detail

**Output parameters:** Medal verdict (SLOP → PLATINUM), pillar scores (SIMPLE, COMPOSABLE, SECURE, NAVIGABLE), ranked refactor targets, assessment status (`IMPROVEMENT`, `REGRESSION`, etc.)

**Other properties:** Writes `.gitnexus` graph artifacts when depgraph is generated; does not modify source files unless the agent chooses to edit based on guidance

## References

- [Topos documentation](https://docs.krv.ai/topos/)
- [Agent contract](https://docs.krv.ai/topos/agents.html)
- [Source repository](https://github.com/Krv-Labs/topos)
- [ClawHub listing](https://clawhub.ai/Krv-Labs/topos)

## Agent Loop

1. **Measure** — `topos evaluate <path> -r` (CLI) or `topos_evaluate_file` / `topos_evaluate_project` (MCP). COMPOSABLE is included by default; run the CLI from the repo root. MCP derives the project from the tool's absolute file/directory path (`TOPOS_MCP_FILE_ROOT` is only an optional maximum boundary). Pass `--gitnexus-dir` / `gitnexus_dir` to select a store; its parent becomes the COMPOSABLE project root.
2. **Inspect** — `topos inspect <file>` or `topos_inspect_code` for per-function complexity and metric detail.
3. **Edit** — one focused structural change (extract helper, simplify branch, decouple import).
4. **Verify** — re-run evaluate, or use `topos_assess_worktree_change` (baseline `HEAD`) for MCP loops. For untracked baselines: `topos_begin_refactor` → edit → `topos_assess_snapshot`.
5. **Behavior check** — run project tests or linters; Topos does not prove correctness.

Stop when the target medal is reached, the priority pillar passes, or further iterations plateau. Prefer structured `agent_contract` fields over parsing prose.

## Lattice (v0.5.0)

- **16 verdicts** on four generators: SIMPLE, COMPOSABLE, SECURE, NAVIGABLE.
- **Medals:** 4 pillars pass → PLATINUM; 3 → GOLD; 2 → SILVER; 1 → BRONZE; 0 → SLOP.
- **`IDEAL` requires all four pillars.** The former three-pillar top verdict is now `SIMPLE_COMPOSABLE_SECURE` (GOLD band). CI or agents pinned to `IDEAL` will fail on NAVIGABLE gate misses.
- **Default ranking:** `SIMPLE ≻ NAVIGABLE ≻ SECURE ≻ COMPOSABLE`. `.topos.toml` and MCP `preferences.ranking` must list all four pillars.

## CLI Reference

| Command | Purpose |
| --- | --- |
| `topos evaluate <path> -r` | Show the cumulative project quality rollup |
| `topos evaluate <path> -r --failures <pillar>` | List the files whose gates fail one pillar |
| `topos evaluate <path> -r --info` | Select a weak file and show ranked line-level refactor targets |
| `topos config show \| set --priority <ranking>` | View or persist project priority and preference settings |
| `topos inspect <file>` | Deep per-file metrics and suggestions |
| `topos compare <a> <b>` | AST edit distance between two versions |
| `topos coverage <source>... --tests <test>... [-r]` | Structural test coverage (UAST + k-gram recall) |
| `topos depgraph generate` | Build GitNexus graph for COMPOSABLE scoring |
| `topos install [--all]` | Register the MCP server in agent harnesses (Claude, Cursor, Codex, …) |
| `topos uninstall [--all]` | Remove Topos-owned MCP entries from harness configs |
| `topos status` | Show which harnesses are configured |
| `topos mcp` | Start the MCP server for tool-based agent loops |

Without `--gitnexus-dir`, COMPOSABLE uses process **cwd** (CLI) or the project derived from the MCP tool's absolute file/directory path, with store at `<project>/.gitnexus`. `TOPOS_MCP_FILE_ROOT` is an optional maximum boundary, not normal editor setup. With `--gitnexus-dir` / `gitnexus_dir`, the store's parent is the COMPOSABLE project root for freshness and `gitnexus analyze` (CLI allows absolute paths outside cwd; MCP requires the store under the derived project). Pass `--no-composable` to disable only COMPOSABLE/MDG scoring; SIMPLE, SECURE, and the AST-derived NAVIGABLE pillar continue to be scored. `topos evaluate` accepts a one-run `--priority` override (a single pillar or a full comma-separated ranking) — `inspect` does not — and `topos config set --priority` persists project defaults; MCP additionally returns the induced preference walk. Advisory `cycles`/`dependencies`/`process` hints are MCP-only, via `topos_refactor`.

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
| `topos_refactor` | Advisory hotspots (`cycles`, `dependencies`, `process`) — never affects the medal |

MCP tool arguments are **flat objects** — `{"filepath": "..."}`, not `{"params": {...}}`.

## Pitfalls

- **No GitNexus → no COMPOSABLE.** The graph is generated automatically, but only if `gitnexus` is installed. If it isn't, `coupling_available` is `false` and PLATINUM is unreachable — check `warnings`.
- **Missing `--gitnexus-dir` from a parent directory → slow COMPOSABLE setup.** Without the override, freshness fingerprints CLI cwd (or the MCP-derived project). Prefer `--gitnexus-dir <repo>/.gitnexus` (or `cd` into the repo) so only that repo is walked. MCP does not need `TOPOS_MCP_FILE_ROOT` for normal editor use.
- **Cosmetic edits don't count.** Whitespace and rename-only changes won't move the lattice; MCP returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.
- **SECURE is structural, not full SAST.** Pair with dedicated security tooling for high-stakes code.
- **`topos_refactor` (MCP-only) is advisory.** It does not replace `topos evaluate` / `topos_evaluate_file` for scoring. There is no `topos refactor` CLI subcommand.

## Verification

A change is ready when:

- Assessment status is `IMPROVEMENT` or `IMPROVEMENT_SCORE` (MCP), or the evaluate verdict improved (CLI).
- Any other status — `LATERAL_MOVE`, `REGRESSION`, `REGRESSION_SCORE`, `SUSPICIOUS_NO_STRUCTURAL_CHANGE` — is not ready.
- Active SECURE findings are fixed or explicitly acknowledged.
- Relevant tests/type checks pass, or their absence is reported.

## Ethical Considerations

Users should review agent-proposed code changes before committing, especially when refactoring production systems. Topos is an advisory structural quality tool; organizations should apply their own security, compliance, and code-review policies before deployment.
