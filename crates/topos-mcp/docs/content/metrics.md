# Topos Metrics Reference

Every metric key, the graph it lives on, and how it rolls into a generator
of `H(G_qual) = { SIMPLE, COMPOSABLE, SECURE }`.

**Calibration source of truth:** `crates/topos-core/src/evaluation/policies/calibration.rs`.
Edit that file when tuning gates or normalization from experimental data.

## SIMPLE generator (← CFG + AST entropy)

Computed from the Control Flow Graph built on UAST.  Always available.

| Key | Source | What it measures | Gate / good range |
|---|---|---|---|
| `cfg.cyclomatic`   | CFG | McCabe complexity `E - N + 2P`.        | **≤ 15** (achieved gate) |
| `cfg.essential`    | CFG | Cabe 1989 essential complexity.        | Diagnostic |
| `cfg.nesting_depth`| CFG | Max static nesting depth.              | Diagnostic |
| `cfg.longest_path` | CFG | Longest acyclic entry-to-exit path.    | Diagnostic |
| `ast.entropy`      | AST | Source-text compression ratio.         | **[0.2, 0.8]** (achieved gate) |
| `ast.max_function_complexity` | AST | Max McCabe of any single function. | **≤ 10** (achieved gate) |

`Φ_SIMPLE` maps metrics to `[0, 1]` quality scores (cyclomatic cap 40,
max-function cap 20, entropy bell peak at 0.5). **`achieved`** is the AND
of the raw gates above — not a single score floor.

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

## Score floors (alternate path)

When callers already hold normalized scores without re-running a `Φᵢ`, the
score-floor dict in `calibration.py` (`SCORE_FLOORS`, re-exported as
`THRESHOLDS` from `policies.base`) applies:

| Generator | Floor |
|---|---|
| SIMPLE | 0.40 |
| COMPOSABLE | 0.80 |
| SECURE | 1.00 |

The live `CharacteristicMorphism` path uses each `Φᵢ`'s `ScoredDecision.achieved`
(raw-metric AND gates), not these floors.

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
