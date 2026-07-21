# AGENTS.md

## Style & Spelling
- **Writing Style**: Always use **American English spelling** ("optimize", "analyze", "modeling").

## Project Architecture
**Topos** evaluates code quality (Python, Rust, JavaScript, TypeScript, C++, Go) using category theory, mapping programs to an 8-element lattice ($\Omega$) of free Heyting algebra on 3 independent, pairwise incomparable generators:
- **`SIMPLE`** (CFG/AST): cyclomatic complexity, nesting, entropy. Passing: $\ge 0.40$.
- **`COMPOSABLE`** (MDG): coupling, instability, fan-in/out. Passing: $\ge 0.80$. Needs a GitNexus module dependency graph (`.gitnexus/`) — `topos evaluate` (CLI) and `topos_evaluate_file`/`topos_evaluate_project` (MCP) all auto-detect and generate/refresh it by default (CLI: `--no-composable`/`--gitnexus-dir`; MCP: `no_composable`/`gitnexus_dir` params). GitNexus missing or generation failing degrades to SIMPLE/SECURE only, never fails the evaluation.
- **`SECURE`** (CPG): dangerous calls, taint flows. Zero-tolerance gates; passing requires a perfect score ($1.00$).
- **Lattice ($\Omega$)**: `SLOP` ($\bot$) < single satisfied generators < dual combinations < `IDEAL` ($\top$). Pointwise meet ($\bigwedge$) for rollups.

### Layout & Extensibility (Rust workspace: `crates/topos-core`, `crates/topos-cli`, `crates/topos-mcp`)
- **`crates/topos-core/src/core/`**: Program category, morphism, objects, `Omega` lattice, and `CharacteristicMorphism` ($\chi_S : P \to \Omega$).
- **`crates/topos-core/src/graphs/`**: Representations implementing the `Representation` trait (`name`, `dimension`, `metrics() -> HashMap<String, f64>`).
- **`crates/topos-core/src/evaluation/policies/`**: gate specs (`gates.rs`), calibration thresholds (`calibration.rs`), and score functions per pillar.
- **`crates/topos-core/src/functors/`**: probes (heavy metrics) and profunctors (pairwise comparisons).

**To Add a Representation**:
1. Create `crates/topos-core/src/graphs/<name>/object.rs` implementing the `Representation` trait, emitting namespaced metrics (e.g. `mdg.*`, `cfg.*`).
2. Add raw metric probes under `crates/topos-core/src/functors/probes/<name>/`.
3. Register the new metric(s) in `GATE_SPECS`/`PILLAR_METRIC_PREFIXES` (`crates/topos-core/src/evaluation/policies/gates.rs`) so gating and prose interpretation pick them up.
4. (Optional) Add pairwise comparison under `crates/topos-core/src/functors/profunctors/<name>/`.

## CLI & Dev Commands
```bash
cargo build --workspace                              # Setup
cargo test --workspace                                # Run tests
cargo fmt --all && cargo clippy --workspace --all-targets  # Lint/format
topos evaluate <path> [-r] [--language <lang>] [--no-composable] [--gitnexus-dir <dir>]
```
`--priority`/`--preferences` are not CLI flags today — priority/preference weighting is only exposed through the MCP tools' `preferences` parameter (see below).

## Weight Control: Priority vs. Preferences
1. **`Priority`** (MCP-only, single knob): upweights the primary metric of a targeted generator (`simple`/`composable`/`secure`).
   - `simple` $\to$ weights: complexity 0.7, other 0.3
   - `composable` $\to$ weights: coupling 0.7, other 0.3
   - `secure` (default) $\to$ weights: taint 0.7, other 0.3
2. **`UserPreferences`** (strict total order over generators, e.g. `[COMPOSABLE, SECURE, SIMPLE]`):
   - Induces a total order on $\Omega$ via integer rank weights `4 / 2 / 1` (most → least preferred).
   - Enables two-stage targeting: target `IDEAL` first, fall back to the meet of the top-2-ranked generators when progress plateaus.
   - Computes the `relaxation_walk` (descending sequence of reachable verdicts) and `next_step` (its smallest-improvement bottom entry).

## MCP Server (`topos-mcp`)
Exposes tools, resources, and prompts for agent workflows:
- **Tools**: `topos_evaluate_code`, `topos_evaluate_file`, `topos_evaluate_project`, `topos_compare_code`, `topos_compare_files`, `topos_assess_improvement` (anti-gaming), `topos_assess_worktree_change` (edit-in-place vs a git ref), `topos_begin_refactor` + `topos_assess_snapshot` (edit-in-place vs a captured baseline), `topos_assess_changeset`, `topos_inspect_code`, `topos_preference_walk`, `topos_calculate_coverage`, `topos_depgraph_status`, `topos_generate_depgraph`, `topos_generate_graphify_graph`, `topos_refactor`, `topos_get_doc`.
- **Resources**: `topos://docs/agent-contract`, `topos://docs/lattice`, `topos://docs/metrics`, `topos://docs/priority`, `topos://docs/preferences`, `topos://docs/workflows`.
- **Prompts**: `topos_refactor_until_ideal`.

## Closed-Loop Agent Workflow
Read `topos://docs/agent-contract` first. Use Topos as the structural verifier:
measure, make one focused structural change, verify with
`topos_assess_worktree_change` for in-place edits, snapshot first only when the
baseline is not in git, and use `topos_assess_improvement` only for side-by-side
variants. Run relevant behavior checks before accepting.
`IMPROVEMENT` / `IMPROVEMENT_SCORE` are Topos acceptance signals, not automatic
commit permission. `SUSPICIOUS_NO_STRUCTURAL_CHANGE` blocks acceptance.

### Escape Hatches
- **Score plateaus**: Split file. Extract high-complexity functions identified by `topos_inspect_code`.
- **SIMPLE improves, COMPOSABLE regresses**: Abstraction is just relocation. Verify whole project rollup.
- **COMPOSABLE still unreachable after evaluating**: GitNexus isn't installed or generation failed — check the `warnings` field (or CLI `stderr`) for why, install GitNexus (`npm install -g gitnexus`) or fix the reported problem, then re-evaluate. `topos_depgraph_status` gives a read-only diagnosis without triggering generation.
