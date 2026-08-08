# Package-level composability — deferred redesign

Status: **DEFERRED until after v0.5.0**. This is a design brief, not an accepted
implementation. The current file-level policy is defined by
[`file-level-composable.md`](file-level-composable.md).

## Current boundary

v0.5.0 gives each file one narrow COMPOSABLE verdict:

```text
file COMPOSABLE achieved ⇔ mdg.fan_out ≤ 10
```

Fan-in, instability, abstractness, main-sequence distance, coupling, and
dependency depth remain inspection context. No package verdict exists, and a
package result must not be inferred from file medals.

## Why a second scope is needed

Martin's `Ca`, `Ce`, instability, abstractness, and distance from the main
sequence are package-design concepts. At file scope their meaning is weak and
their ratios are often quantized by very small edge counts. That makes them
useful for diagnosis, but unsuitable as file-level hard gates.

A future design may evaluate package stability, but it must produce a separate
package-scoped result. Copying one package verdict onto every member file would
multiply one observation through project statistics and make file medals depend
on boundaries the file cannot control.

## Required redesign

### 1. Define package identity explicitly

Package identity should come from ecosystem boundaries such as `Cargo.toml`,
`pyproject.toml`, or `package.json`, with documented handling for workspaces and
monorepos. Directory depth or adaptive "merge until measurable" rules are not
stable identities and are easy to manipulate by moving files.

If Topos cannot identify a defensible package boundary, the package measure must
abstain rather than invent one.

### 2. Recompute boundary coupling

Package `Ca` and `Ce` must be computed from graph edges crossing the package
boundary. Intra-package edges are excluded and far ends are deduplicated at the
chosen package scope. Member-file readings cannot be summed because that would
double-count internal relationships.

The design must specify separately how `IMPORTS` and `CALLS` edges contribute,
how external dependencies without repository nodes are represented, and how
generated or vendored code is treated.

### 3. Introduce a scope-aware result contract

Add package results alongside file results, with at least:

- a stable `scope_id` and package root;
- availability/confidence and an abstention reason;
- raw package metrics and calibrated verdicts;
- member-file attribution for the edges driving a poor result.

The existing file COMPOSABLE verdict remains file-scoped. Package stability does
not become another bit in a file medal by inheritance.

### 4. Preserve actionable attribution

Package findings must rank the member files and boundary edges that contribute
most to the result. Agents edit files, so a package verdict without concrete
file/edge evidence is not an actionable refactor target.

### 5. Make proposed-code evaluation honest

`topos_assess_improvement` currently evaluates dependency metrics against a
GitNexus snapshot. A full redesign must either refresh that graph, support a
correct incremental symbol-resolution update, or mark package evidence stale.
It must not infer cross-file edges from imports alone and present them as
resolved call relationships.

### 6. Calibrate and validate at the new scope

File thresholds do not transfer to packages. Before gating, collect a
multi-ecosystem package corpus and publish distributions by ecosystem and
package size. More importantly, validate candidate measures against outcomes
such as defect proneness, change propagation, or review effort. A percentile
alone can set an operational release threshold; it cannot establish construct
validity.

### 7. Version the contract deliberately

Package results change CLI/MCP schemas, aggregation, badges, and leaderboard
semantics. Introduce them additively and experimentally before making any
package gate normative.

## Recommended sequence

1. Emit experimental package identities and raw boundary metrics.
2. Audit identity and aggregation on polyglot monorepos.
3. Add member-file/edge attribution and stale-graph signaling.
4. Calibrate distributions and test external validity.
5. Decide whether package stability warrants a separate verdict.

Until those steps are complete, v0.5.0's file fan-out gate is the only
COMPOSABLE verdict.
