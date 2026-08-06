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
`topos/engine/src/evaluation/policies/gates.rs`; `cfg.cyclomatic` is the
only spec with `gates_achieved: false`.)

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
| `mdg.instability` | `Ce / (Ca + Ce)`.                        | **[0.3, 0.7]** — only gated when `mdg.abstractness` is unavailable for the file's language (see below) |
| `mdg.abstractness` | Fraction of the module's type declarations that are abstract (trait/interface/protocol/abstract class) vs. concrete (struct/class/enum). `0.0` for a functions-only module (no type declarations at all) — that is a real, meaningful "fully concrete" reading, not "unmeasured." | Diagnostic |
| `mdg.main_sequence_distance` | Martin's Distance from the Main Sequence, `D = \|A + I − 1\|`. Replaces the raw `mdg.instability` gate whenever abstractness is available — a concrete, unstable orchestrator (I≈1, A≈0, e.g. `main.rs`) sits *on* the main sequence (D≈0) and is not penalized, unlike a fixed instability band. | **≤ 0.5** (achieved gate, when active) |
| `mdg.fan_in`      | Incoming `CALLS` edges.                  | **≤ 15** (achieved gate) |
| `mdg.fan_out`     | Outgoing `CALLS` edges.                  | **≤ 15** (achieved gate) |
| `mdg.dep_depth`   | Longest `IMPORTS` chain.                 | Diagnostic |

**Why two instability gates?** Gating raw `mdg.instability` against a
fixed band flags both stable leaf modules (constants, error types) and
unstable orchestrators (`main.rs`, bootstrap/wiring code) even when those
extremes are architecturally intentional — see issue #124. Pairing
instability with Abstractness and gating on distance from the main
sequence fixes this for languages where Abstractness is measured; other
languages (currently: JavaScript, C++) keep the original band gate
unchanged until their UAST mappers gain type-declaration classification.
A separate role-based exemption (`is_stable_leaf_module` — a
declarations-only module with no branching control flow) tolerates
maximal main-sequence distance for frozen, concrete foundation/utility
code, mirroring Martin's own accepted "Zone of Pain" exception.

`Φ_COMPOSABLE` uses fan caps of 40 for score normalization. **`achieved`**
is the AND of whichever instability-family gate is active, plus fan-in and
fan-out.

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
| `nav.max_function_divergence` | Worst function's Semantic Compositional Divergence — `Σ depth(u)·ln(1 + fanout(u))` over the scope-forming nodes inside it. | **≤ 8.0** (achieved gate) |

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
fail for its length. `Φ_NAVIGABLE` decays linearly to a cap of 20.0 for
the reported score.

When the gate fails, `metric_locations["nav.max_function_divergence"]`
carries the offending functions worst-first with real spans, so the failure
becomes a `refactor_target`. The fix is `extract_helper`: lift the deepest
nested block into a top-level function.

> **PROVISIONAL calibration.** The `8.0` gate and `20.0` score cap are
> first-pass estimates, not yet run through the corpus ECDF calibration the
> SIMPLE and SECURE constants received. On Topos's own 157 Rust files the
> distribution is median `0.69`, p75 `2.08`, p90 `4.56`, p95 `5.98` — so
> `8.0` currently sits near p97 and fails 2.5% of files. Expect this
> constant to move.

## Score floors (alternate path)

When callers already hold normalized scores without re-running a `Φᵢ`, the
`score_floor(generator)` function in
`topos/engine/src/evaluation/policies/calibration.rs` applies:

| Generator | Floor |
|---|---|
| SIMPLE | 0.40 |
| COMPOSABLE | 0.80 |
| SECURE | 1.00 |
| NAVIGABLE | 0.40 (PROVISIONAL) |

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
