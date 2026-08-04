# AGENTS.md

## Style & Spelling
- **Writing Style**: Always use **American English spelling** ("optimize", "analyze", "modeling").

## Project Architecture
**Topos** evaluates code quality (Python, Rust, JavaScript, TypeScript, C++, Go) using category theory, mapping programs to a 16-element lattice ($\Omega$) of free Heyting algebra on 4 independent, pairwise incomparable generators ($|\Omega| = 2^{\texttt{GENERATOR\_COUNT}}$, `core/omega.rs`):
- **`SIMPLE`** (CFG/AST): cyclomatic complexity, nesting, entropy. Passing: $\ge 0.40$. Gates on `ast.max_function_complexity` (a true per-function max) and `ast.entropy`; `cfg.cyclomatic` is scored but **advisory only** (`gates_achieved: false`) because it is a whole-file merged-CFG sum that scales with function count.
- **`COMPOSABLE`** (MDG): coupling, instability, fan-in/out. Passing: $\ge 0.80$. Pairing instability with abstractness (`mdg.abstractness`), it gates on Distance from the Main Sequence (`mdg.main_sequence_distance = |A + I - 1| \le 0.5`) for supported languages when coupling signal exists, falling back to raw instability when no coupling data or abstractness support exists. Needs a GitNexus module dependency graph (`.gitnexus/`) — `topos evaluate` (CLI) and `topos_evaluate_file`/`topos_evaluate_project` (MCP) all auto-detect and generate/refresh it by default (CLI: `--no-composable`/`--gitnexus-dir`; MCP: `no_composable`/`gitnexus_dir` params). GitNexus missing or generation failing degrades to the other pillars only, never fails the evaluation.
- **`SECURE`** (CPG): dangerous calls, taint flows. Zero-tolerance gates; passing requires a perfect score ($1.00$). SECURE scoring stays CPG-native; the embedded Sighthound SAST engine only supplies supplementary, per-finding `security_findings` detail (advisory-only).
- **`NAVIGABLE`** (UAST): how expensive a file is for an agent to hold in its head. Gates on the **per-function max** Semantic Compositional Divergence, `nav.max_function_divergence` $= \sum_u \mathrm{depth}(u)\cdot\ln(1 + \mathrm{fanout}(u))$ over scope-forming nodes inside each callable. Passing: $\ge 0.40$. Measures *nesting*, not branch count — a flat function scores `0.0` however many branches it has, because branch count is already SIMPLE's concern. Read from the same UAST as SIMPLE, so it needs **no external tooling** and is always available across all six languages. **Thresholds are `PROVISIONAL`** — see [#282](https://github.com/Krv-Labs/topos/issues/282).
- **Lattice ($\Omega$)**: `SLOP` ($\bot$) < single satisfied generators < pairs < triples < `IDEAL` ($\top$, all four). Pointwise meet ($\bigwedge$) for rollups. **`IDEAL` requires all four generators** — the former three-generator top is now `SIMPLE_COMPOSABLE_SECURE`.
- **Medal tiers** band on satisfied-generator count, derived from `satisfied_count()` rather than a parallel table: 4 → `PLATINUM`, 3 → `GOLD`, 2 → `SILVER`, 1 → `BRONZE`, 0 → `SLOP`. `EvaluationValue::medal_tier()` is the single source — renderers must call it, never re-derive the popcount thresholds.

### Layout & Extensibility (Rust workspace: `topos/engine` (crate `topos-engine`), `topos/cli` (crate `topos`), `topos/mcp` (crate `topos-mcp`))
- **`topos/engine/src/core/`**: Program category, morphism, objects, `Omega` lattice, and `CharacteristicMorphism` ($\chi_S : P \to \Omega$).
- **`topos/engine/src/graphs/`**: Representations implementing the `Representation` trait (`name`, `dimension`, `metrics() -> HashMap<String, f64>`).
- **`topos/engine/src/evaluation/policies/`**: gate specs (`gates.rs`), calibration thresholds (`calibration.rs`), and one score function per pillar (`simple.rs`, `composable.rs`, `secure.rs`, `navigable.rs`).
- **`topos/engine/src/functors/`**: probes (heavy metrics) and profunctors (pairwise comparisons). NAVIGABLE's probe is `probes/ast/divergence.rs`, with scope-walking shared in `probes/ast/scopes.rs`.
- **`topos/engine/src/adapters/`**: external tools and integrations (`gitnexus.rs`, `graphify.rs`, `process.rs`).
- **`topos/engine/src/config.rs`**: `.topos.toml` configuration parsing and allowlist rules.

**To Add a Representation**:
1. Create `topos/engine/src/graphs/<name>/object.rs` implementing the `Representation` trait, emitting namespaced metrics (e.g. `mdg.*`, `cfg.*`).
2. Add raw metric probes under `topos/engine/src/functors/probes/<name>/`.
3. Register the new metric(s) in `GATE_SPECS`/`PILLAR_METRIC_PREFIXES` (`topos/engine/src/evaluation/policies/gates.rs`) so gating and prose interpretation pick them up.
4. (Optional) Add pairwise comparison under `topos/engine/src/functors/profunctors/<name>/`.

**To Add a Generator to $\Omega$** (rarer — last done for `NAVIGABLE`): bump `GENERATOR_COUNT` in `core/omega.rs` (`OMEGA_SIZE` and the medal banding follow from it), add the variant to `Generator`, `Priority` and `score_floor`, add a `policies/<name>.rs` translator, wire it into `CharacteristicMorphism`, and widen the preference ranking. **This is a breaking change**: `IDEAL` gains a requirement, medals re-grade, and every schema carrying a verdict or ranking changes shape.

## CLI & Dev Commands
```bash
cargo build --workspace                              # Setup
cargo test --workspace                                # Run tests
cargo fmt --all && cargo clippy --workspace --all-targets  # Lint/format

# CLI Subcommands:
topos evaluate <path> [-r] [--language <lang>] [--no-composable] [--gitnexus-dir <dir>]
                      [--priority <pillar|ranking>] [--info] [--failures <pillar>] [--json] [-v]
topos config [set ...]                               # View or edit .topos.toml (priority)
topos inspect <path>                                 # Detailed metrics
topos compare <path1> <path2>                         # Structural distance
topos coverage --put <path1> --test <path2>           # UAST test coverage
topos graphify generate|orphans                      # Graphify integration
topos depgraph generate [--force]                     # GitNexus generation
topos mcp                                             # Launch MCP server over stdio
```
`--priority` accepts either a single pillar (`simple`/`composable`/`secure`/`navigable`) or a full comma-separated ranking of **all four**, and `topos config set` writes the same value to `.topos.toml` under one `priority` key. MCP exposes the equivalent via the `preferences` parameter (see below).

## Priority → Total Order → Relaxation Walk

**Priority exists to induce a total order on $\Omega$**, so an agent gets an unambiguous relaxation walk that respects what the user cares about. $\Omega$ is only a *partial* order — the generators are pairwise incomparable, so `SIMPLE` and `SECURE` are not comparable on their own. A ranking is what makes "which verdict is better" answerable, and therefore what makes "what should I fix next" answerable.

The order is always carried by **`UserPreferences`**, never by `Priority` itself. `Priority` is the single-pillar shorthand and the output label; it is *lifted into* a ranking to do any ordering work.

```text
--priority secure           (single pillar)
  → focused_ranking(Secure, configured)      promote to front, rest keep configured order
  → UserPreferences [SECURE, SIMPLE, COMPOSABLE, NAVIGABLE]
  → score(v) = Σ 1 << (RANKING_LEN-1-rank) over satisfied g    → 8 / 4 / 2 / 1
  → induced_total_order()                    all 16 elements, strictly ranked
  → relaxation_walk(current) / next_step(current)
```

1. **`UserPreferences`** — a strict total order over **all four** generators, e.g. `[COMPOSABLE, SECURE, SIMPLE, NAVIGABLE]`:
   - `score()` sums `1 << (RANKING_LEN - 1 - rank)` over the satisfied generators — weights `8 / 4 / 2 / 1` (most → least preferred). Powers of two, so the order is strictly lexicographic: no combination of lower-ranked generators can outrank a higher-ranked one. Adding a generator doubles the top weight automatically, which is why `RANKING_LEN` is the only thing to change.
   - Two-stage targeting: `aspirational_target()` is `IDEAL`; `fallback_target()` guarantees the top-two ranked generators and concedes the rest, for when progress plateaus. **At four generators these are no longer adjacent** — at three they coincided, so code that assumed "one step below IDEAL is the fallback" is now wrong.
   - `relaxation_walk(current)` is the descending sequence of reachable verdicts that still outrank `current`; `next_step(current)` is its bottom entry — the smallest improvement worth making.
   - Default ranking is `(SIMPLE, NAVIGABLE, SECURE, COMPOSABLE)` — the two agent-cognition pillars first (both UAST-derived, always available, fixable inside one file), then zero-tolerance `SECURE`, then `COMPOSABLE` **last** because it is the least locally actionable: it needs an external GitNexus graph and describes a module's place in the whole dependency graph. Ranking it last means the walk concedes it first, which is right when `coupling_available` is `false`. Under this default the fallback target is `SIMPLE_NAVIGABLE` and one step below `IDEAL` is `SIMPLE_SECURE_NAVIGABLE`.
   - A ranking listing fewer than four generators is not a permutation of `G_qual`; it is **dropped** and the default applies, matching the best-effort contract other malformed `.topos.toml` keys get.
2. **`Priority`** — names one emphasized generator (`simple`/`composable`/`secure`/`navigable`). It carries **no weights**; `Priority::top_generator()` is its whole behavior. It does not rescale any metric.
   - **CLI**: `--priority <pillar>` is lifted via `focused_ranking`, so a single pillar yields a full ranking and a real total order. `--priority <a,b,c,d>` supplies the ranking directly.
   - **MCP**: `priority` is **output-only** (a label in the result, plus `priority_source`). Input takes `preferences` only, and `resolve_priority` derives the label *from* that ranking. So an MCP agent must send all four generators to get a priority-respecting walk — there is no single-pillar shorthand on the wire, unlike the CLI.
   - Caveat: engine `Priority::default()` is `Secure`, but MCP's `resolve_priority(None)` returns `Simple` with `PrioritySource::Default`. Read `priority_source` rather than assuming a default.

> Do **not** reintroduce metric weighting here. The old `WeightProfile` (per-pillar `0.7 / 0.3` metric weights) was deleted rather than grown a fourth field — gates are independent thresholds, and scores are the min of per-metric qualities. Ordering belongs in the ranking, not in the scorer.

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
- **SIMPLE passes but NAVIGABLE fails**: the function's *branch count* is fine, its *nesting* is not — flattening is the fix, not splitting decisions apart. Invert conditions to early-return, or extract the deepest nested block (`extract_helper`). A file-wide sum would not tell you which function; `nav.max_function_divergence` resolves to the offending functions worst-first with line spans.
- **NAVIGABLE regresses while SIMPLE improves**: extracting a helper *into* an existing nested block adds a level. Extract to module scope instead.
- **COMPOSABLE still unreachable after evaluating**: GitNexus isn't installed or generation failed — check the `warnings` field (or CLI `stderr`) for why, install GitNexus (`pnpm add -g gitnexus` or `npm install -g gitnexus`) or fix the reported problem, then re-evaluate. `topos_depgraph_status` gives a read-only diagnosis without triggering generation.
