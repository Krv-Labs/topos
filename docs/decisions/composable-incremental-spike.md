# Incremental COMPOSABLE evaluation — deferred

Status: **DEFERRED after v0.5.0**. The current implementation evaluates
dependency metrics against a GitNexus graph snapshot. This record defines when
that approximation should be replaced.

## Current behavior

`topos_assess_improvement` parses proposed source locally, but cross-file
dependency evidence still comes from the existing `ModuleDependencyGraph`.
Topos reports graph availability and staleness rather than claiming that the
snapshot reflects unindexed edits.

For the current file-level verdict, the important distinction is:

- `mdg.fan_out` counts resolved external-symbol `CALLS` edges and is the sole
  gate (`fan_out ≤ 10`);
- `mdg.fan_in` counts inbound `CALLS` edges and is advisory;
- coupling and instability come from `IMPORTS` relationships and are diagnostic
  or advisory.

Changing an import does not by itself prove a new call edge. Consequently, an
import-only AST patch cannot correctly update the gating fan-out measure.

## Decision

Do not build a partial in-memory graph patch that updates imports while leaving
symbol resolution and calls stale. It would create precise-looking but
internally inconsistent evidence.

Prefer these options in order:

1. Regenerate GitNexus when its cost is acceptable.
2. Surface a clear stale-graph warning and abstain when freshness matters.
3. Build incremental graph updates only if they reproduce full GitNexus symbol
   resolution for the affected edges.

## Requirements for an incremental implementation

A correct implementation would need:

- cross-language import and call-reference extraction;
- first-party module and symbol resolution matching GitNexus behavior;
- a supported replacement operation for one file's outgoing graph edges;
- consistent incoming/outgoing indexes after replacement;
- explicit handling of unresolved and third-party targets;
- equivalence tests against a fresh full `gitnexus analyze` run.

The baseline graph must remain immutable while evaluating proposed code, and
the API must identify which readings came from refreshed, patched, or stale
evidence.

## Measurement gate

Build this only after real refactor sessions show that stale graph evidence
materially changes agent decisions or crosses the `fan_out ≤ 10` verdict boundary
often enough to justify owning an incremental resolver. Until then, transparent
staleness plus full regeneration has a better correctness/cost tradeoff.
