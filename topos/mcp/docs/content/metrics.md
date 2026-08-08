# Topos Metrics Reference

Every metric key, the graph it lives on, and how it rolls into a generator
of `H(G_qual) = { SIMPLE, COMPOSABLE, SECURE, NAVIGABLE }`.

**Calibration source of truth:** `topos/engine/src/evaluation/policies/calibration.rs`.
Edit that file when tuning gates or normalization from experimental data.

## SIMPLE generator (← CFG + AST entropy)

Computed from the Control Flow Graph built on UAST.  Always available.

| Key | Source | What it measures | Gate / good range |
|---|---|---|---|
| `cfg.cyclomatic`   | CFG | McCabe complexity `E - N + 2P`.        | **≤ 15** — advisory |
| `cfg.essential`    | CFG | Cabe 1989 essential complexity.        | Diagnostic |
| `cfg.nesting_depth`| CFG | Max static nesting depth.              | Diagnostic |
| `cfg.longest_path` | CFG | Longest acyclic entry-to-exit path.    | Diagnostic |
| `ast.entropy`      | AST | Source-text compression ratio.         | **[0.2, 0.8]** (achieved gate) |
| `ast.max_function_complexity` | AST | Max McCabe of any single function. | **≤ 10** (achieved gate) |

*Column legend.* **achieved gate** — a violation fails the pillar's
`achieved`. **advisory** — scored (it shapes `score` and drives
`extract_helper` / `split_decision_logic` suggestions) but never fails
`achieved`. **Diagnostic** — reported in `raw_metrics` only.

**Why `cfg.cyclomatic` is advisory (issue #193).** It is a *whole-file
merged-CFG* sum, so it scales with function count: a file of many small,
individually-simple functions would hard-fail on it for no real
complexity reason. `ast.max_function_complexity` — a true per-function
max — gates that concern directly, so cyclomatic keeps its `≤ 15`
reference point for interpretation and scoring only. (Source of truth:
`GateSpec::gates_achieved` in
`topos/engine/src/evaluation/policies/gates.rs`.)

`Φ_SIMPLE` maps metrics to `[0, 1]` quality scores (cyclomatic cap 40,
max-function cap 20, entropy bell peak at 0.5), then takes `score =
min(qualities)`. **`achieved`** is the AND of exactly two raw gates —
`ast.entropy` in band (or entrypoint-exempt) **and**
`ast.max_function_complexity ≤ 10` — not a single score floor, and *not*
a conjunction over every row above. Because `score` is a minimum over
all scored metrics including the advisory one, a file can legitimately
report `achieved: true` with `score: 0.0`: e.g. a large
dispatch/discovery-class module with `cfg.cyclomatic: 63` (past the cap
of 40 → quality 0.0) whose individual functions all stay under 10.

## COMPOSABLE generator (← Dependency Graph + UAST Abstractness)

`mdg.instability`/`mdg.coupling`/`mdg.fan_in`/`mdg.fan_out`/`mdg.dep_depth`
require a `ModuleDependencyGraph` parsed from `.gitnexus/`. `topos_evaluate_file`/
`topos_evaluate_project` generate/refresh that graph automatically by
default (missing or stale → run `gitnexus analyze`), so these populate
without any extra call; pass `gitnexus_dir` to point at a specific graph,
or `no_composable: true` to skip detection/generation entirely.
`mdg.abstractness` is UAST-derived and needs no GitNexus directory — it is
available whenever the language's UAST mapper classifies type
declarations (Python, Rust, Go, TypeScript today; not JavaScript, which
has no abstract-type concept).

| Key | What it measures | Gate / good range |
|---|---|---|
| `mdg.coupling`    | Ca + Ce (afferent + efferent coupling).  | Diagnostic |
| `mdg.instability` | `Ce / (Ca + Ce)`; dependency role/direction. | **[0.3, 0.7]** — advisory |
| `mdg.abstractness` | Fraction of type declarations that are abstract. | Diagnostic input to distance |
| `mdg.main_sequence_distance` | Martin's package-oriented `D = \|A + I − 1\|`, projected onto the file for architectural context. | **≤ 0.5** — advisory |
| `mdg.fan_in`      | Incoming `CALLS` edges; responsibility/change-impact radius. | **≤ 15** — advisory |
| `mdg.fan_out`     | Distinct external symbols called by the file; outward interaction burden. | **≤ 10** (achieved gate) |
| `mdg.dep_depth`   | Longest `IMPORTS` chain.                 | Diagnostic |

**Why only fan-out gates at file scope?** Outward interaction is a direct local
burden: it approximates how much external behavior a file must coordinate.
High fan-in instead marks impact radius and can be correct for an interface or
shared utility. Martin instability and main-sequence distance describe package
roles; sparse file graphs also make their attainable values coarse. They remain
scored, interpreted, and available as refactor targets, but do not hard-fail a
file. See the cited rationale and calibration in
`docs/decisions/file-level-composable.md`.

`Φ_COMPOSABLE` uses fan caps of 40 for score normalization and takes the minimum
over all scored readings, including advisory ones. **`achieved`** is determined
only by `mdg.fan_out <= 10`. Consequently, an achieved file may still have a low
COMPOSABLE score that invites deeper inspection.

## SECURE generator (← Code Property Graph)

Computed from a CPG fused over AST + CFG + DDG + CDG (Yamaguchi et al.,
arxiv:1909.03496).  Always available.

| Key | What it measures | Gate |
|---|---|---|
| `cpg.dangerous_calls` | Count of reachable call sites whose callee matches the per-language dangerous-API registry (Python: `eval`, `exec`, `pickle.loads`, `subprocess.*(shell=True)`, ...; C++: `gets`, `strcpy`, ...). | **0** (strict) |
| `cpg.taint_flows`     | DDG paths from any taint source (e.g. `input`, `request.args`) to any dangerous-API sink. | **0** (strict) |

`Φ_SECURE` decays exponentially in both counts (scale 3.0 each) for the
reported score. **`achieved`** requires zero dangerous calls and zero taint flows.

File-level MCP tools also surface `security_findings` with `kind`, `callee`,
`line`, and `snippet` when SECURE fails.  Project scans keep this off by default
unless `include_security_findings=true`.

## NAVIGABLE generator (← AST scope tree)

Computed from the same UAST as SIMPLE, so it needs no external input and
is always available.

| Key | What it measures | Gate |
|---|---|---|
| `nav.max_function_divergence` | Worst function's Semantic Compositional Divergence — `Σ depth(u)·ln(1 + fanout(u))` over the scope-forming nodes inside it. | **≤ 10.0** (achieved gate) |

**What this is for.** Once code length is controlled for, classical
complexity metrics stop predicting LLM task accuracy but *nesting depth*
keeps predicting it: each level is another hierarchical state a reader has
to hold open. NAVIGABLE measures nesting and nothing else.

Two consequences of the formula are load-bearing:

- A leaf scope contributes `ln(1) = 0`, so a **perfectly flat function
  scores `0.0`** regardless of how many branches it has. Flat *is*
  maximally navigable; branch count is SIMPLE's concern. Deep code is
  still fully counted — the weight lands on the ancestors doing the
  nesting.
- Ternaries and short-circuit boolean operators are **excluded**.
  Expression-level branching opens no block, so it costs no reader state,
  and counting it here would just re-measure SIMPLE.

Like `ast.max_function_complexity`, the gate is the **per-function max**
rather than a file-wide sum — a long file of short flat functions must not
fail for its length. `Φ_NAVIGABLE` decays linearly to a cap of 12.0 for
the reported score.

When the gate fails, `metric_locations["nav.max_function_divergence"]`
carries the offending functions worst-first with real spans, so the failure
becomes a `refactor_target`. The fix is `extract_helper`: lift the deepest
nested block into a top-level function.

> **Calibration.** The `10.0` gate was selected from a balanced 6,390-file
> leaderboard corpus (p95 `10.37`, ~5.2% gate failure rate). The
> `12.0` score cap spans p99 across Rust (`10.40`), Go (`13.64`), and Python
> (`12.31`) so scores decay linearly without early flooring. Topos's 176 Rust
> sources remain the reference ECDF (p95 `5.65`, p99 `8.62`, max `12.19`).

## Score floors (alternate path)

When callers already hold normalized scores without re-running a `Φᵢ`, the
`score_floor(generator)` function in
`topos/engine/src/evaluation/policies/calibration.rs` applies:

| Generator | Floor |
|---|---|
| SIMPLE | 0.40 |
| COMPOSABLE | 0.80 |
| SECURE | 1.00 |
| NAVIGABLE | 0.40 |

The live `CharacteristicMorphism` path uses each `Φᵢ`'s `ScoredDecision.achieved`
(the AND of that generator's *gating* raw metrics — advisory ones excluded),
not these floors.

## Diagnostic-only metrics (academic PDG)

The intra-procedural Program Dependence Graph emits diagnostic metrics
that surface in `raw_metrics` but do not drive a generator:

| Key | What it measures |
|---|---|
| `pdg.data_deps`    | Count of DDG edges (def→use chains). |
| `pdg.control_deps` | Count of CDG edges (predicate→executor). |
| `pdg.density`      | `(data + control) / statement_count`. |

## Priority weights

The `priority` parameter shifts weights *within* each `Φᵢ` — it does not
change the lattice structure.

| Priority | `w_complexity` | `w_coupling` | `w_taint` |
|---|---|---|---|
| `simple`     | 0.7 | 0.3 | 0.3 |
| `composable` | 0.3 | 0.7 | 0.3 |
| `secure`     | 0.3 | 0.3 | 0.7 |

See `topos://docs/priority` for how to pick one.

## Anti-gaming guardrail

`topos_assess_improvement` flags `SUSPICIOUS_NO_STRUCTURAL_CHANGE` when
scores move ≥ 3 percentage points but the normalized AST edit distance is
< 0.02.  Catches agents that "improve" scores via whitespace shuffles,
comment edits, or renames that don't change the tree.
