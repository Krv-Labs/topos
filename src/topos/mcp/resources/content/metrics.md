# Topos Metrics Reference

All metric keys, what they mean, and how they roll up into dimensions.

## Structural dimension (→ SELF_CONTAINED)

Computed from the AST alone. Always available.

| Key | What it measures | Good range | Interpretation |
|---|---|---|---|
| `ast.complexity` | Sum of cyclomatic complexities across functions. | ≤ 40 per file | Higher → more branches, harder to reason about. |
| `ast.entropy` | Compression ratio of the source text. | around 0.5 | Too low → repetitive boilerplate. Too high → obfuscated / dense code. |

**Structural score** = weighted average of `1 - complexity/40` and a
bell-curve over entropy (peak at 0.5). Threshold for SELF_CONTAINED: **0.6**.

## Coupling dimension (→ COMPOSABLE)

Requires a `DependencyGraph` (parsed from `.gitnexus/`). Only populated when
`gitnexus_dir` is provided or auto-detected.

| Key | What it measures | Good range | Interpretation |
|---|---|---|---|
| `depgraph.coupling` | Ca + Ce (afferent + efferent coupling). | ≤ 35 | Total module fan-in/out. |
| `depgraph.instability` | Ce / (Ca + Ce). | 0.3 – 0.7 | Near 0: stable/dependent. Near 1: abstract/unstable. Sweet spot in the middle. |
| `depgraph.fan_in` | Incoming `CALLS` edges. | — | How many things depend on this file. |
| `depgraph.fan_out` | Outgoing `CALLS` edges. | — | How many things this file depends on. |
| `depgraph.dep_depth` | Longest `IMPORTS` chain. | — | Transitive reach. |

**Coupling score** = weighted average of `1 - coupling/35` and instability
quality. Threshold for COMPOSABLE: **0.6**.

## Priority weights

The `priority` parameter shifts weights *within* each dimension — it does
not change the lattice structure.

| Priority | `w_complexity` | `w_coupling` |
|---|---|---|
| `balanced` | 0.5 | 0.5 |
| `self_contained` | 0.7 | 0.3 |
| `composable` | 0.3 | 0.7 |

See `topos://docs/priority` for how to pick one.

## Anti-gaming guardrail

`topos_assess_improvement` flags `SUSPICIOUS_NO_STRUCTURAL_CHANGE` when
scores move ≥ 3 percentage points but the normalized AST edit distance is
< 0.02. This catches agents that "improve" scores via whitespace shuffles,
comment edits, or renames that don't change the tree.
