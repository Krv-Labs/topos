# Topos Metrics Reference

Every metric key, the graph it lives on, and how it rolls into a generator
of `H(G_qual) = { SIMPLE, COMPOSABLE, SECURE }`.

**Calibration source of truth:** `topos/evaluation/policies/calibration.py`.
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

## COMPOSABLE generator (← Dependency Graph)

Requires a `ModuleDependencyGraph` parsed from `.gitnexus/`.  Only populated when
`gitnexus_dir` is provided or auto-detected.

| Key | What it measures | Gate / good range |
|---|---|---|
| `mdg.coupling`    | Ca + Ce (afferent + efferent coupling).  | Diagnostic |
| `mdg.instability` | `Ce / (Ca + Ce)`.                        | **[0.3, 0.7]** (achieved gate) |
| `mdg.fan_in`      | Incoming `CALLS` edges.                  | **≤ 15** (achieved gate) |
| `mdg.fan_out`     | Outgoing `CALLS` edges.                  | **≤ 15** (achieved gate) |
| `mdg.dep_depth`   | Longest `IMPORTS` chain.                 | Diagnostic |

`Φ_COMPOSABLE` uses fan caps of 40 for score normalization. **`achieved`**
is the AND of instability band + fan-in + fan-out gates.

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
