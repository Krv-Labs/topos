# Topos Metrics Reference

Every metric key, the graph it lives on, and how it rolls into a generator
of `H(G_qual) = { SIMPLE, COMPOSABLE, SECURE }`.

## SIMPLE generator (← CFG + AST entropy)

Computed from the Control Flow Graph built on UAST.  Always available.

| Key | Source | What it measures | Good range |
|---|---|---|---|
| `cfg.cyclomatic`   | CFG | McCabe complexity `E - N + 2P`.        | ≤ 40 per file |
| `cfg.essential`    | CFG | Cabe 1989 essential complexity.        | Low |
| `cfg.nesting_depth`| CFG | Max static nesting depth.              | ≤ 4 |
| `cfg.longest_path` | CFG | Longest acyclic entry-to-exit path.    | — |
| `ast.entropy`      | AST | Source-text compression ratio.         | around 0.5 |

`Φ_SIMPLE` = weighted average of `1 - cyclomatic/40` and the entropy bell
curve (peak at 0.5).  Threshold to satisfy SIMPLE: **0.6**.

## COMPOSABLE generator (← Dependency Graph)

Requires a `ModuleDependencyGraph` parsed from `.gitnexus/`.  Only populated when
`gitnexus_dir` is provided or auto-detected.

| Key | What it measures | Good range |
|---|---|---|
| `mdg.coupling`    | Ca + Ce (afferent + efferent coupling).  | ≤ 35 |
| `mdg.instability` | `Ce / (Ca + Ce)`.                        | 0.3 – 0.7 |
| `mdg.fan_in`      | Incoming `CALLS` edges.                  | — |
| `mdg.fan_out`     | Outgoing `CALLS` edges.                  | — |
| `mdg.dep_depth`   | Longest `IMPORTS` chain.                 | — |

`Φ_COMPOSABLE` = weighted average of `1 - coupling/35` and the instability
tent over `[0.3, 0.7]`.  Threshold: **0.6**.

## SECURE generator (← Code Property Graph)

Computed from a CPG fused over AST + CFG + DDG + CDG (Yamaguchi et al.,
arxiv:1909.03496).  Always available.

| Key | What it measures |
|---|---|
| `cpg.dangerous_calls` | Count of reachable call sites whose callee matches the per-language dangerous-API registry (Python: `eval`, `exec`, `pickle.loads`, `subprocess.*(shell=True)`, ...; C++: `gets`, `strcpy`, ...). |
| `cpg.taint_flows`     | DDG paths from any taint source (e.g. `input`, `request.args`) to any dangerous-API sink. |

`Φ_SECURE` decays exponentially in both counts.  Threshold: **0.6**.
File-level MCP tools also surface `security_findings` with `kind`, `callee`,
`line`, and `snippet` when SECURE fails.  Project scans keep this off by default
unless `include_security_findings=true`.

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
